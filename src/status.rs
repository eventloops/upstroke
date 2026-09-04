//! Extended notes: `docs/internals/status.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::RunReport;
use crate::error::UpstrokeError;
use crate::events::{self, Event, EventBody, LogTail, RunStarted, RunState};
use crate::interaction::Sleeper;
use crate::ir::Plan;
use crate::rundir::{self, RunPaths};

mod render;

pub struct RunStatus {
    pub run_id: String,
    pub paths: RunPaths,
    pub started: RunStarted,
    pub state: RunState,
    pub plan: Plan,
    pub running: bool,
    pub held: bool,
    pub interrupted: u32,
    pub warnings: Vec<String>,
}

impl RunStatus {
    pub fn report(&self) -> RunReport {
        RunReport::from_state(
            &self.started,
            &self.plan,
            &self.state,
            self.warnings.clone(),
            self.running,
            self.interrupted_run(),
        )
    }

    pub fn interrupted_run(&self) -> bool {
        !self.running && self.state.finished.is_none()
    }
}

fn husk_answer(repo_root: &Path, wanted: &str) -> Option<UpstrokeError> {
    let husk_id = rundir::list_husks(repo_root)
        .into_iter()
        .find(|id| id.eq_ignore_ascii_case(wanted))?;
    let repo_key = rundir::RepoKey::for_repo(repo_root).ok()?;
    let report = rundir::husk_report(
        repo_root,
        &husk_id,
        &repo_key,
        &rundir::default_private_root(),
    );
    let locator = report.locator.as_ref().map_or_else(
        || " It records no private locator.".to_owned(),
        |path| format!(" Its private locator is {}.", path.display()),
    );
    Some(UpstrokeError::Refused {
        message: format!(
            "run `{husk_id}` never recorded a committed run_started: it is {}.{locator}",
            report.disposition.describe()
        ),
    })
}

pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {
    let run_id = match run_id {
        Some(wanted) => match rundir::resolve_run_id(repo_root, wanted) {
            Ok(resolved) => resolved,
            Err(error) => return Err(husk_answer(repo_root, wanted).unwrap_or(error)),
        },
        None => rundir::latest_run(repo_root).ok_or_else(|| UpstrokeError::Refused {
            message: format!(
                "no runs found under {} — nothing has run in this repository yet",
                rundir::runs_root(repo_root).display()
            ),
        })?,
    };
    let public = rundir::public_dir(repo_root, &run_id);
    let events_path = public.join("events.jsonl");

    let (bytes, held) = stable_event_bytes_with(
        &events_path,
        || events::read_bytes(&events_path),
        || rundir::is_running(&public),
    )?;
    let parsed = events::parse_bytes(&events_path, &bytes)?;
    let mut warnings = Vec::new();
    warnings.extend(parsed.torn_tail_warning);
    let events = parsed.events;
    let started = events::started_of(&events, &events_path)?.clone();
    let effective_schema = events::ensure_supported_schema(&started, &events, &events_path)?;
    if started.run_id != run_id {
        return Err(UpstrokeError::EventLog {
            path: events_path.clone(),
            message: format!(
                "run_started id `{}` does not match directory `{run_id}`",
                started.run_id
            ),
        });
    }
    let paths = RunPaths::from_parts(public.clone(), PathBuf::from(&started.private_dir));

    let plan_path = paths.plan_json();
    let plan_bytes = std::fs::read(&plan_path).map_err(|source| UpstrokeError::Io {
        path: plan_path.clone(),
        source,
    })?;
    if effective_schema >= 3 {
        let recorded = events::recorded_normalized_plan_digest(&events).ok_or_else(|| {
            UpstrokeError::EventLog {
                path: events_path.clone(),
                message: "event schema 3 does not record the normalized-plan SHA-256 digest"
                    .to_owned(),
            }
        })?;
        let actual = events::normalized_plan_digest(&plan_bytes);
        if actual != recorded {
            return Err(UpstrokeError::EventLog {
                path: plan_path.clone(),
                message: format!(
                    "normalized plan digest `{actual}` does not match recorded digest `{recorded}`"
                ),
            });
        }
    }
    let plan: Plan = serde_json::from_slice(&plan_bytes).map_err(|e| UpstrokeError::Parse {
        message: format!("{}: {e}", plan_path.display()),
    })?;
    if plan.source.hash != started.plan_hash {
        return Err(UpstrokeError::EventLog {
            path: plan_path.clone(),
            message: format!(
                "frozen plan hash `{}` does not match run-start hash `{}`",
                plan.source.hash, started.plan_hash
            ),
        });
    }

    let task_ids = plan.tasks.iter().map(|task| task.id.to_string()).collect();
    let mut replayed = events::replay(events, task_ids, &events_path)?;
    let running = held && replayed.state.finished.is_none();
    let interrupted = if running {
        0
    } else {
        replayed.state.settle_interrupted()
    };

    Ok(RunStatus {
        run_id,
        paths,
        started: replayed.started,
        state: replayed.state,
        plan,
        running,
        held,
        interrupted,
        warnings,
    })
}

pub fn render(status: &RunStatus) -> String {
    render::render(status)
}

pub fn describe(event: &Event) -> String {
    render::describe(event)
}

pub fn follow(
    status: &RunStatus,
    sleeper: &dyn Sleeper,
    poll: Duration,
    max_idle_polls: u32,
    out: &mut dyn std::io::Write,
) -> Result<(), UpstrokeError> {
    let mut tail = LogTail::new(status.paths.events());
    let mut warnings = Vec::new();
    let mut idle = 0;
    let mut terminal = false;
    loop {
        let events = tail.poll(&mut warnings)?;
        if events.is_empty() {
            let running = rundir::is_running(&status.paths.public);
            if terminal && !running {
                return Ok(());
            }
            if running {
                idle = 0;
            } else {
                idle += 1;
                if idle > max_idle_polls {
                    return Ok(());
                }
            }
            sleeper.sleep(poll);
            continue;
        }
        idle = 0;
        for event in &events {
            let _ = writeln!(out, "{}", describe(event));
            match &event.body {
                EventBody::RunFinished { .. } => terminal = true,
                EventBody::RunResumed { .. } => terminal = false,
                _ => {}
            }
        }
        if terminal && !rundir::is_running(&status.paths.public) {
            return Ok(());
        }
    }
}

fn stable_event_bytes_with(
    path: &Path,
    mut read: impl FnMut() -> Result<Vec<u8>, UpstrokeError>,
    mut held: impl FnMut() -> bool,
) -> Result<(Vec<u8>, bool), UpstrokeError> {
    const MAX_SNAPSHOT_ATTEMPTS: usize = 8;
    for _ in 0..MAX_SNAPSHOT_ATTEMPTS {
        let held_before = held();
        let first = read()?;
        let held_after = held();
        if held_before != held_after {
            continue;
        }
        if held_after {
            return Ok((first, true));
        }

        let second = read()?;
        let held_final = held();
        if !held_final && first == second {
            return Ok((second, false));
        }
    }
    Err(UpstrokeError::Refused {
        message: format!(
            "{} kept changing while status checked whether its engine was live; retry status once the transition settles",
            path.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{
        AttemptRecord, AttemptStarted, AttemptTransition, LadderEscalated, LadderRetry,
        RunFinished, RunOutcome, TaskCommitted, TaskDeferred, TaskFailed,
    };
    use crate::ir::{Answer, Question, QuestionId, QuestionKind, TaskId};
    use crate::ladder::{FailureKind, FailureOrigin};

    fn event(body: EventBody) -> Event {
        Event {
            ts: "2026-08-09T14:03:07Z".to_owned(),
            body,
        }
    }

    #[test]
    fn status_asked_for_a_husk_id_names_which_husk_it_is() {
        let root = std::env::temp_dir().join(format!(
            "upstroke-status-husk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q", "-b", "main"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git init");

        let husk = "01STATUSHUSK00000000000000";
        std::fs::create_dir_all(rundir::public_dir(&root, husk)).expect("husk");
        let Err(error) = load(&root, Some(husk)) else {
            panic!("a husk is not a run and status must not load one");
        };
        let said = error.to_string();
        assert!(said.contains(husk), "names the id: {said}");
        assert!(
            said.contains("never recorded a committed run_started"),
            "says why: {said}"
        );
        assert!(
            said.contains("unstarted husk"),
            "and which of the three it is: {said}"
        );
        assert!(
            said.contains("records no private locator"),
            "and its locator, or that there is none: {said}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_live_to_dead_transition_retries_instead_of_settling_a_stale_prefix() {
        use std::collections::VecDeque;

        let mut reads = VecDeque::from([
            b"attempt-started\n".to_vec(),
            b"attempt-started\nattempt-finished\n".to_vec(),
            b"attempt-started\nattempt-finished\n".to_vec(),
        ]);
        let mut probes = VecDeque::from([true, false, false, false, false]);
        let (bytes, held) = stable_event_bytes_with(
            Path::new("events.jsonl"),
            || Ok(reads.pop_front().expect("bounded read sequence")),
            || probes.pop_front().expect("bounded probe sequence"),
        )
        .expect("the second snapshot is stable");

        assert!(!held);
        assert_eq!(bytes, b"attempt-started\nattempt-finished\n");
        assert!(reads.is_empty());
        assert!(probes.is_empty());
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
                    adapter: None,
                    preflight_cli_version: None,
                    effort: None,
                    selection_origin: None,
                    tier: "small".to_owned(),
                    agent: "claude-code".to_owned(),
                    model: "claude-haiku-4-5".to_owned(),
                    pool: Some("claude-max".to_owned()),
                    resume_session: None,
                },
            }),
            event(EventBody::TaskCommitted {
                task: "t1".to_owned(),
                data: TaskCommitted {
                    sha: "0123456789abcdef".to_owned(),
                    message: "[upstroke] t1: do it".to_owned(),
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
        assert!(line.contains("upstroke answer q-01ABC"), "{line}");
    }

    #[test]
    fn describe_atomic_attempt_transitions() {
        let cases = [
            (
                AttemptTransition::Retry(LadderRetry {
                    resume: true,
                    tier: "mid".to_owned(),
                    summary: "try again".to_owned(),
                    detail: None,
                }),
                "retrying on mid in the same session",
            ),
            (
                AttemptTransition::Escalate(LadderEscalated {
                    to_rung: 2,
                    tier: "frontier".to_owned(),
                    summary: "go higher".to_owned(),
                    detail: None,
                }),
                "escalating past frontier to rung 2",
            ),
            (
                AttemptTransition::Defer(TaskDeferred {
                    reason: "pool unavailable".to_owned(),
                    defers: 3,
                }),
                "deferred (3) — pool unavailable",
            ),
            (
                AttemptTransition::Fail(TaskFailed {
                    kind: FailureKind::GateFailed,
                    reason: "gates exhausted".to_owned(),
                    halts_run: true,
                }),
                "task failed (GateFailed)",
            ),
        ];

        for (transition, expected) in cases {
            let line = describe(&event(EventBody::AttemptFinished {
                task: "t1".to_owned(),
                attempt: 1,
                rung: 0,
                profile: "implement".to_owned(),
                data: Box::new(AttemptRecord {
                    attempt: 1,
                    tier: "small".to_owned(),
                    model: "model".to_owned(),
                    pool: None,
                    resumed: false,
                    duration: Duration::from_secs(1),
                    cost_usd: None,
                    reviews: Vec::new(),
                    session_id: None,
                    usage: None,
                    failure: Some(events::FailureRecord {
                        kind: FailureKind::GateFailed,
                        origin: FailureOrigin::Worker,
                        reason: "the attempt failed".to_owned(),
                        detail: None,
                    }),
                }),
                parking: None,
                transition: Some(Box::new(transition)),
                prepared_commit: None,
            }));
            assert!(line.contains(expected), "{line}");
        }
    }

    #[test]
    fn describe_composes_escalation_with_spend_approval_parking() {
        let line = describe(&event(EventBody::AttemptFinished {
            task: "t1".to_owned(),
            attempt: 1,
            rung: 0,
            profile: "implement".to_owned(),
            data: Box::new(AttemptRecord {
                attempt: 1,
                tier: "small".to_owned(),
                model: "model".to_owned(),
                pool: None,
                resumed: false,
                duration: Duration::from_secs(1),
                cost_usd: None,
                reviews: Vec::new(),
                session_id: None,
                usage: None,
                failure: Some(events::FailureRecord {
                    kind: FailureKind::GateFailed,
                    origin: FailureOrigin::Worker,
                    reason: "the attempt failed".to_owned(),
                    detail: None,
                }),
            }),
            parking: Some(Box::new(events::AttemptParking {
                question: Question {
                    id: QuestionId::from("q-spend"),
                    kind: QuestionKind::ApproveSpend,
                    affected_tasks: vec![TaskId::from("t1")],
                    context: "approve the next rung".to_owned(),
                    options: Vec::new(),
                },
                refund_attempt: false,
            })),
            transition: Some(Box::new(AttemptTransition::Escalate(LadderEscalated {
                to_rung: 1,
                tier: "small".to_owned(),
                summary: "escalate".to_owned(),
                detail: None,
            }))),
            prepared_commit: None,
        }));
        assert!(line.contains("escalating past small to rung 1"), "{line}");
        assert!(line.contains("parked on question q-spend"), "{line}");
        assert_eq!(line.lines().count(), 1, "{line:?}");
    }

    #[test]
    fn the_ledger_keeps_worker_and_review_spend_apart() {
        let report = RunReport {
            run_id: "01RUN".to_owned(),
            branch: "upstroke/run-01RUN".to_owned(),
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
                review_models: vec!["claude-opus-5".to_owned()],
                review_cost_usd: Some(0.05),
                review_cost_incomplete: false,
                session_id: None,
                attempts: vec![AttemptRecord {
                    attempt: 1,
                    tier: "small".to_owned(),
                    model: "claude-haiku-4-5".to_owned(),
                    pool: Some("claude-max".to_owned()),
                    resumed: false,
                    duration: Duration::from_secs(3),
                    cost_usd: Some(0.01),
                    reviews: Vec::new(),
                    session_id: None,
                    usage: None,
                    failure: None,
                }],
            }],
            halted_at: None,
            questions: Vec::new(),
            budget_stop: None,
            running: false,
            interrupted: false,
            total_cost_usd: 0.06,
            pool_drain: vec![crate::engine::PoolDrainRow {
                pool: "claude-max".to_owned(),
                attempts: 1,
                cost_usd: Some(0.01),
                unpriced: 0,
            }],
        };
        let ledger = report.render_ledger();
        assert!(ledger.contains("worker"), "{ledger}");
        assert!(ledger.contains("$0.0100"), "implementer's own spend");
        assert!(ledger.contains("$0.0500"), "reviewer's, kept apart");
        assert!(ledger.contains("$0.0600"), "and the total");
        assert!(ledger.contains("per-pool drain:"), "{ledger}");
        assert!(
            ledger.contains("claude-max: 1 attempt(s), $0.0100"),
            "{ledger}"
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
                review_models: Vec::new(),
                review_cost_usd: None,
                review_cost_incomplete: false,
                session_id: None,
                attempts: Vec::new(),
            }],
            halted_at: None,
            questions: Vec::new(),
            budget_stop: None,
            running: false,
            interrupted: false,
            total_cost_usd: 0.0,
            pool_drain: Vec::new(),
        };
        let ledger = report.render_ledger();
        assert!(
            ledger.contains('—'),
            "unreported must not read as $0.0000: {ledger}"
        );
    }

    #[test]
    fn answers_and_defects_render_without_quoting_the_operator() {
        let line = describe(&event(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: QuestionId::from("q-1"),
                answer: Answer::Answered {
                    text: "\u{1b}[31mnot a control sequence\u{1b}[0m".to_owned(),
                },
                decline_halts_run: None,
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
