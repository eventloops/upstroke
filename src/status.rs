//! `upstroke status` — the run, folded out of its own log (DESIGN.md §15).
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
// LEGACY-EFFECT: this module is in the **frozen legacy section** of
// `effects/allowlist.toml`, which carries its justification and the condition
// under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::engine::RunReport;
use crate::error::UpstrokeError;
use crate::events::{self, Event, EventBody, LogTail, RunStarted, RunState};
use crate::interaction::Sleeper;
use crate::ir::Plan;
use crate::rundir::{self, RunPaths};

/// The view and the per-event lines, which reach nothing and so restore the
/// effect denials this module allows.
mod render;

/// One run, as read back from disk.
pub struct RunStatus {
    pub run_id: String,
    pub paths: RunPaths,
    pub started: RunStarted,
    pub state: RunState,
    pub plan: Plan,
    /// Whether an engine is driving this run *and* the run has not recorded
    /// that it finished — the two halves of "still going", which the lock
    /// alone does not answer.
    pub running: bool,
    /// Whether anything holds the run's lock, finished or not.
    ///
    /// Kept beside `running` rather than folded into it because a process
    /// claiming a run that already ended is real and worth saying: `resume`
    /// takes the lock and holds it across a dozen git subprocesses before it
    /// writes `run_resumed`. During that window the run has an owner and an
    /// outcome at the same time, and an operator asking `status` deserves
    /// both.
    pub held: bool,
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
            self.running,
            self.interrupted_run(),
        )
    }

    /// Whether this run stopped without recording that it had finished — the
    /// signature of a kill, a power loss, or an aborting error.
    pub fn interrupted_run(&self) -> bool {
        !self.running && self.state.finished.is_none()
    }
}

/// What `status` says about a husk id, or `None` if `wanted` names no husk.
///
/// Read-only end to end, and it never resolves a husk into a run: the answer
/// is the refusal, carrying which of the three kinds of husk this is, its
/// reason and its private locator. The authorized private root is the default
/// one, which is the root a read-only command is configured with.
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

/// Load a run: the newest one, or any unambiguous id prefix.
pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {
    let run_id = match run_id {
        Some(wanted) => match rundir::resolve_run_id(repo_root, wanted) {
            Ok(resolved) => resolved,
            // `startup_census`: "status is read-only: it ignores husks and,
            // asked explicitly for a husk id, reports an unstarted husk that
            // the next write command reclaims, a retained husk with its reason
            // and locator, or a possibly committed run whose public log has no
            // valid committed first line".
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
    // Two questions, not one. The lock says whether a process has claimed this
    // run; the log says whether the run still has anywhere to go. `running`
    // needs both, for the same reason `interrupted_run` below does: `resume`
    // claims the lock before it writes anything, so a budget-stopped run has an
    // owner for as long as that resume takes to get going. Reading the lock
    // alone made those seconds render as `run in progress`, dropping the stop
    // reason, the parked list, and the `resume --budget` line the operator is
    // there to find.
    let running = held && replayed.state.finished.is_none();
    // Settled in memory only: status is a pure read and must not write to a
    // run it is merely looking at. A resume records the same settlement as
    // events instead.
    //
    // And only for a run nothing is driving. An attempt in flight under a live
    // engine has not been interrupted — it is working — so settling it here
    // would report a running attempt as a failure and the whole run as halted.
    // `status` is the only window into a run that holds its own terminal, so
    // that reading is worse than no reading at all.
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

/// The whole view: what happened, what it cost, and what it is waiting for.
///
/// The view itself is the private `render` child's; this is the public
/// surface it is reached through.
pub fn render(status: &RunStatus) -> String {
    render::render(status)
}

/// One human line per event, for `--follow`.
///
/// Renders the supplied event without folding or validating a log. Recorded
/// control characters are made safe for a terminal; malformed timestamps and
/// leap-second values retain their full date. A supplied duration is shown at
/// millisecond precision. See DESIGN.md §18 for the output contract.
pub fn describe(event: &Event) -> String {
    render::describe(event)
}

/// Stream a run's events, from the beginning and then as they arrive.
///
/// Starting from the beginning is deliberate: `--follow` on a run already in
/// progress should show how it got here, not drop the reader into the middle
/// of a story. Reads only whole lines, so a follower attached to a live engine
/// never sees half an event. Returns once the run records that it is done —
/// or, once nothing is driving the run any more, after `max_idle_polls` with
/// nothing new, so a follower attached to a run whose engine has died gives up
/// instead of waiting forever.
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
            // The idle budget is not a timeout on silence. A whole attempt —
            // the agent's thinking, its tool calls, the gates, the review —
            // folds into a single `attempt_finished`, so a healthy run says
            // nothing for minutes at a time; giving up on one would drop the
            // live view mid-run. The budget exists only to release a terminal
            // attached to an engine that has died, so it starts counting when
            // the run's lock does not.
            //
            // One syscall per poll, asked plainly. This used to need a cheaper
            // variant of its own, because the check waited out a contention
            // grace every time the answer was yes — which on a healthy run is
            // every poll. The lock now answers exactly, so there is no cheaper
            // question to ask.
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
        // A resume owns the lock before it can append RunResumed. A follower
        // that sees the previous epoch's RunFinished in that window must wait
        // for the marker rather than treating historical terminal state as the
        // current process's result.
        if terminal && !rundir::is_running(&status.paths.public) {
            return Ok(());
        }
    }
}

/// Pair event bytes with a stable liveness observation. A dead snapshot is
/// trusted only after an identical second read and a second dead probe; this
/// prevents status from reading `attempt_started`, observing the conductor
/// release its lock after writing the settlement, and then inventing an
/// interrupted attempt from the stale prefix.
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
        ReviewPassOutcome, ReviewRecord, RunFinished, RunOutcome, RunResumed, TaskCommitted,
        TaskDeferred, TaskFailed, TaskParked,
    };
    use crate::ir::{Answer, Question, QuestionId, QuestionKind, TaskId};
    use crate::ladder::{FailureKind, FailureOrigin};

    fn event(body: EventBody) -> Event {
        Event {
            ts: "2026-08-09T14:03:07Z".to_owned(),
            body,
        }
    }

    /// `load` composes `resolve_run_id`'s refusal with `rundir::husk_report`,
    /// and a composition nobody drives is the shape `PR4-CONF-008` was: both
    /// halves were tested and their join was not. So this asks `status` itself.
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
        // A real repository, because the husk answer takes this repository's
        // key over its canonical common git dir.
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
        assert!(rendered[0].starts_with("14:03:07Z  "), "{:?}", rendered[0]);
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
        // §13's second currency, beside the dollars and derived from the same
        // attempt records.
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
        // The operator's words are an instruction to the agent, not something
        // status needs to echo into a terminal it does not control.
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

    /// One settled record, varying only what a test is about.
    fn record(failure: Option<events::FailureRecord>, reviews: Vec<ReviewRecord>) -> AttemptRecord {
        AttemptRecord {
            attempt: 1,
            tier: "small".to_owned(),
            model: "model".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::from_secs(1),
            cost_usd: None,
            reviews,
            session_id: None,
            usage: None,
            failure,
        }
    }

    fn gate_failure(reason: &str) -> events::FailureRecord {
        events::FailureRecord {
            kind: FailureKind::GateFailed,
            origin: FailureOrigin::Worker,
            reason: reason.to_owned(),
            detail: None,
        }
    }

    fn review(pass: &str, outcome: ReviewPassOutcome) -> ReviewRecord {
        ReviewRecord {
            pass: pass.to_owned(),
            agent: "codex".to_owned(),
            model: "gpt-5".to_owned(),
            adapter: None,
            preflight_cli_version: None,
            effort: None,
            pool: None,
            cost_usd: None,
            outcome,
        }
    }

    fn finished(
        record: AttemptRecord,
        parking: Option<events::AttemptParking>,
        transition: Option<AttemptTransition>,
    ) -> Event {
        event(EventBody::AttemptFinished {
            task: "t1".to_owned(),
            attempt: 1,
            rung: 0,
            profile: "implement".to_owned(),
            data: Box::new(record),
            parking: parking.map(Box::new),
            transition: transition.map(Box::new),
            prepared_commit: None,
        })
    }

    fn parking(id: &str) -> events::AttemptParking {
        events::AttemptParking {
            question: Question {
                id: QuestionId::from(id),
                kind: QuestionKind::Unblock,
                affected_tasks: vec![TaskId::from("t1")],
                context: "every rung failed".to_owned(),
                options: Vec::new(),
            },
            refund_attempt: false,
        }
    }

    fn answered(answer: Answer, decline_halts_run: Option<bool>) -> Event {
        event(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: QuestionId::from("q-1"),
                answer,
                decline_halts_run,
                via: "terminal".to_owned(),
            },
        })
    }

    /// The pre-repair line for every answer was "q-1 answered via terminal":
    /// a decline, which fails the affected task and may halt the run, and a
    /// question no channel could deliver both read as an answer.
    #[test]
    fn a_decline_is_described_as_a_decline_with_the_halt_policy_frozen_with_it() {
        let halting = describe(&answered(Answer::Declined, Some(true)));
        assert!(
            halting.contains(
                "q-1 declined via terminal; the halt policy frozen with it says the run halts"
            ),
            "{halting}"
        );
        assert!(!halting.contains("answered"), "{halting}");

        let continuing = describe(&answered(Answer::Declined, Some(false)));
        assert!(
            continuing.contains(
                "q-1 declined via terminal; the halt policy frozen with it says the run continues"
            ),
            "{continuing}"
        );

        let legacy = describe(&answered(Answer::Declined, None));
        assert!(
            legacy.contains("the halt policy frozen with it was not recorded"),
            "{legacy}"
        );

        let nobody = describe(&answered(Answer::Unanswered, None));
        assert!(
            nobody.contains("q-1 went unanswered via terminal: no channel reached a person"),
            "{nobody}"
        );
        assert!(!nobody.contains("q-1 answered"), "{nobody}");

        let answer = describe(&answered(
            Answer::Answered {
                text: "go on".to_owned(),
            },
            None,
        ));
        assert!(answer.contains("q-1 answered via terminal"), "{answer}");
    }

    #[test]
    fn a_decline_line_does_not_invent_a_failure_before_its_event() {
        for halts_run in [false, true] {
            let mut state = RunState::new(vec!["t1".to_owned(), "t2".to_owned()]);
            state.apply(&event(EventBody::QuestionRaised {
                task: "t1".to_owned(),
                data: Box::new(events::QuestionRaised {
                    question: Question {
                        id: QuestionId::from("q-1"),
                        kind: QuestionKind::Unblock,
                        affected_tasks: vec![TaskId::from("t1"), TaskId::from("t2")],
                        context: "both tasks need the same decision".to_owned(),
                        options: Vec::new(),
                    },
                }),
            }));
            for task in ["t1", "t2"] {
                state.apply(&event(EventBody::TaskParked {
                    task: task.to_owned(),
                    data: TaskParked {
                        question: "q-1".to_owned(),
                        refund_attempt: false,
                    },
                }));
            }
            // The coordinator writes the answer before either task failure.
            // A crash here leaves this prefix as the entire durable truth.
            let decline = answered(Answer::Declined, Some(halts_run));
            state.apply(&decline);
            assert_eq!(state.questions.len(), 1);
            assert_eq!(state.questions[0].answer, Some(Answer::Declined));
            assert_eq!(state.states.len(), 2);
            assert!(state.states.iter().all(|task| {
                *task == events::TaskState::AwaitingInput(QuestionId::from("q-1"))
            }));
            assert_eq!(state.halted_at, None);
            assert_eq!(state.finished, None);
            let line = describe(&decline);
            assert!(line.contains("q-1 declined via terminal"), "{line}");
            assert!(line.contains("halt policy frozen with it"), "{line}");
            assert!(!line.contains("task fails"), "{line}");
            assert!(!line.contains("task failed"), "{line}");

            for task in ["t1", "t2"] {
                let failure = event(EventBody::TaskFailed {
                    task: task.to_owned(),
                    data: TaskFailed {
                        kind: FailureKind::Declined,
                        reason: "declined at the human rung".to_owned(),
                        halts_run,
                    },
                });
                state.apply(&failure);
                let line = describe(&failure);
                assert!(line.contains(&format!("{task}: task failed")), "{line}");
            }
            assert!(state.states.iter().all(|task| {
                matches!(
                    task,
                    events::TaskState::Failed {
                        kind: FailureKind::Declined,
                        ..
                    }
                )
            }));
            assert_eq!(state.halted_at.as_deref(), halts_run.then_some("t1"));
        }
    }

    #[test]
    fn settled_status_sanitizes_the_report_and_its_resume_and_answer_lines() {
        let hostile = "x\n\u{1b}[2Jy";
        let mut state = RunState::new(vec!["t1".to_owned()]);
        state.apply(&event(EventBody::TaskFailed {
            task: "t1".to_owned(),
            data: TaskFailed {
                kind: FailureKind::AgentError,
                reason: hostile.to_owned(),
                halts_run: false,
            },
        }));
        state.apply(&event(EventBody::QuestionRaised {
            task: "t1".to_owned(),
            data: Box::new(events::QuestionRaised {
                question: Question {
                    id: QuestionId::from(hostile),
                    kind: QuestionKind::Unblock,
                    affected_tasks: vec![TaskId::from("t1")],
                    context: "still needs an answer".to_owned(),
                    options: Vec::new(),
                },
            }),
        }));
        let status = RunStatus {
            run_id: hostile.to_owned(),
            paths: RunPaths::from_parts(PathBuf::from("public"), PathBuf::from(hostile)),
            started: RunStarted {
                schema: 3,
                upstroke_version: "0.1.0".to_owned(),
                run_id: hostile.to_owned(),
                branch: "upstroke/run-1".to_owned(),
                base_sha: "a".repeat(40),
                plan_path: "plan.md".to_owned(),
                config_path: None,
                plan_hash: "hash".to_owned(),
                normalized_plan_digest: None,
                private_dir: hostile.to_owned(),
                gates: Vec::new(),
                gates_from_config: false,
                interaction_mode: "on_block".to_owned(),
                chains: Vec::new(),
                effort_policy: None,
                gate_cmds: None,
                reviews: None,
            },
            state,
            plan: Plan {
                source: crate::ir::PlanSource {
                    adapter: "markdown".to_owned(),
                    hash: "hash".to_owned(),
                },
                tasks: vec![crate::ir::Task {
                    id: TaskId::from("t1"),
                    kind: crate::ir::TaskKind::Fix,
                    title: "repair the task".to_owned(),
                    body: String::new(),
                    depends_on: Vec::new(),
                    acceptance: Vec::new(),
                    path_hints: Vec::new(),
                    suggested_tier: None,
                    min_tier: None,
                    artifacts_in: Vec::new(),
                    artifacts_out: Vec::new(),
                }],
                artifacts: Vec::new(),
            },
            running: false,
            held: false,
            interrupted: 0,
            warnings: vec![hostile.to_owned()],
        };
        let rendered = render(&status);
        assert!(
            rendered.chars().all(|c| c == '\n' || !c.is_control()),
            "{rendered:?}"
        );
        assert!(
            rendered.contains("FAILED [] — x \\u{1b}[2Jy"),
            "{rendered:?}"
        );
        assert!(
            rendered.contains("Continue it with:\n    upstroke resume x \\u{1b}[2Jy\n"),
            "{rendered:?}"
        );
        assert!(
            rendered.contains("    upstroke answer x \\u{1b}[2Jy\n"),
            "{rendered:?}"
        );
        assert!(
            rendered.contains("transcripts: x \\u{1b}[2Jy\n"),
            "{rendered:?}"
        );
    }

    #[test]
    fn a_finished_run_names_its_outcome_in_words_and_the_task_it_halted_at() {
        let finished = |outcome, halted_at: Option<&str>| {
            describe(&event(EventBody::RunFinished {
                data: RunFinished {
                    outcome,
                    halted_at: halted_at.map(str::to_owned),
                    committed: 1,
                    parked: 0,
                },
            }))
        };
        let halted = finished(RunOutcome::Halted, Some("t2"));
        assert!(
            halted.contains("run finished: halted at `t2` (1 committed, 0 parked)"),
            "{halted}"
        );
        let budget = finished(RunOutcome::BudgetExceeded, None);
        assert!(
            budget.contains("run finished: stopped at its budget (1 committed, 0 parked)"),
            "{budget}"
        );
        let complete = finished(RunOutcome::Complete, None);
        assert!(
            complete.contains("run finished: complete (1 committed, 0 parked)"),
            "{complete}"
        );
        assert!(
            !complete.contains("Complete"),
            "a derived Debug spelling is not a contract: {complete}"
        );
        // The parser admits both inconsistent shapes; each is shown as what
        // the record says rather than folded into a halt or a silence.
        let unnamed = finished(RunOutcome::Halted, None);
        assert!(
            unnamed.contains(
                "run finished: halted at a task the record does not name (1 committed, 0 parked)"
            ),
            "{unnamed}"
        );
        let odd = finished(RunOutcome::Complete, Some("t2"));
        assert!(
            odd.contains(
                "run finished: complete (`t2` recorded as halted_at on a run that did not halt) \
                 (1 committed, 0 parked)"
            ),
            "{odd}"
        );
        assert!(!odd.contains("complete at"), "{odd}");
    }

    /// The record production writes for a rejected review carries both the
    /// pass's `Failed` outcome and a `ReviewFailed` failure whose reason is
    /// `review failed: …` (`engine::attempt::evaluate_review`); an unavailable
    /// reviewer carries `Unavailable` and a `ReviewUnavailable` failure. The
    /// line names the pass and the model beside the reason on that shape — the
    /// one a run produces — and not only on a record with the outcome alone.
    #[test]
    fn a_real_review_rejection_names_the_pass_and_the_model_beside_its_reason() {
        let rejected = describe(&finished(
            record(
                Some(events::FailureRecord {
                    kind: FailureKind::ReviewFailed,
                    origin: FailureOrigin::Reviewer,
                    reason: "review failed: no tests were added".to_owned(),
                    detail: None,
                }),
                vec![
                    review("review", ReviewPassOutcome::Passed),
                    review("second-opinion", ReviewPassOutcome::Failed),
                ],
            ),
            None,
            Some(AttemptTransition::Retry(LadderRetry {
                resume: false,
                tier: "small".to_owned(),
                summary: "review failed: no tests were added".to_owned(),
                detail: None,
            })),
        ));
        assert!(
            rejected.contains(
                "t1: attempt 1 failed — review failed: no tests were added; review \
                 `second-opinion` (gpt-5) rejected it; retrying on small"
            ),
            "{rejected}"
        );
        let unavailable = describe(&finished(
            record(
                Some(events::FailureRecord {
                    kind: FailureKind::ReviewUnavailable,
                    origin: FailureOrigin::Reviewer,
                    reason: "reviewer unavailable: rate limited".to_owned(),
                    detail: None,
                }),
                vec![review("review", ReviewPassOutcome::Unavailable)],
            ),
            None,
            None,
        ));
        assert!(
            unavailable.contains(
                "t1: attempt 1 failed — reviewer unavailable: rate limited; review `review` \
                 (gpt-5) reached no verdict"
            ),
            "{unavailable}"
        );
        // A gate failure has no review to name, and says nothing about one.
        let gates = describe(&finished(
            record(Some(gate_failure("gate `test` failed: exit 1")), Vec::new()),
            None,
            None,
        ));
        assert!(
            gates.contains("t1: attempt 1 failed — gate `test` failed: exit 1"),
            "{gates}"
        );
        assert!(!gates.contains("review"), "{gates}");
    }

    #[test]
    fn a_deferral_wait_says_how_long_and_which_round() {
        for (waited, expected, round) in [
            (Duration::from_millis(999), "0.999", 2),
            (Duration::ZERO, "0.000", 0),
            (Duration::from_millis(1), "0.001", 1),
            (Duration::from_secs(1), "1.000", 3),
            (Duration::from_millis(1001), "1.001", 4),
            (Duration::from_secs(90), "90.000", 5),
            // Above f64's exact-integer range, milliseconds still survive.
            (
                Duration::from_millis(9_007_199_254_740_993),
                "9007199254740.993",
                6,
            ),
            (Duration::from_millis(u64::MAX), "18446744073709551.615", 7),
            // Public describe also accepts values beyond the wire's u64 ms.
            (Duration::MAX, "18446744073709551615.999", u32::MAX),
            (Duration::from_nanos(999_999), "0.000", 9),
        ] {
            let line = describe(&event(EventBody::DeferWaitElapsed {
                data: events::DeferWaitElapsed { waited, round },
            }));
            assert_eq!(
                line,
                format!("14:03:07Z  waited {expected}s for a pool to come back (round {round})")
            );
        }
    }

    #[test]
    fn a_terminal_failure_says_whether_the_run_halts_in_both_wire_shapes() {
        for (halts_run, expected) in [(true, "; the run halts"), (false, "; the run continues")] {
            let failed = TaskFailed {
                kind: FailureKind::GateFailed,
                reason: "gates exhausted".to_owned(),
                halts_run,
            };
            // Independent events own equal transition snapshots, so exercise
            // both wire shapes with the same failure policy and reason.
            let standalone = describe(&event(EventBody::TaskFailed {
                task: "t1".to_owned(),
                data: failed.clone(),
            }));
            for parked in [None, Some(parking("q-1"))] {
                let expected_parking = if parked.is_some() {
                    "; parked on question q-1"
                } else {
                    ""
                };
                // Public describe can receive either parking shape; its
                // transition must retain reason B beside attempt reason A.
                let atomic = describe(&finished(
                    record(Some(gate_failure("the attempt failed")), Vec::new()),
                    parked,
                    Some(AttemptTransition::Fail(failed.clone())),
                ));
                assert_eq!(
                    atomic,
                    format!(
                        "14:03:07Z  t1: attempt 1 failed — the attempt failed; task failed (GateFailed) — gates exhausted{expected}{expected_parking}"
                    )
                );
            }
            assert!(
                standalone.contains(&format!(
                    "t1: task failed (GateFailed) — gates exhausted{expected}"
                )),
                "{standalone}"
            );
        }
    }

    #[test]
    fn a_resume_that_discarded_edits_says_how_many() {
        let resumed = |discarded: &[&str]| {
            describe(&event(EventBody::RunResumed {
                data: RunResumed {
                    head_sha: "0123456789abcdef".to_owned(),
                    interrupted_attempts: 1,
                    discarded: discarded.iter().map(|path| (*path).to_owned()).collect(),
                    gates: None,
                    effort_policy: None,
                    reviews: None,
                    chains: None,
                    normalized_plan_digest: None,
                },
            }))
        };
        let discarding = resumed(&["src/a.rs", "src/b.rs"]);
        assert!(
            discarding.contains(
                "resumed at 0123456789 (1 interrupted attempt(s)); 2 uncommitted path(s) discarded"
            ),
            "{discarding}"
        );
        let clean = resumed(&[]);
        assert!(
            clean.contains("resumed at 0123456789 (1 interrupted attempt(s))"),
            "{clean}"
        );
        assert!(!clean.contains("discarded"), "{clean}");
    }

    /// "Passed" follows `AttemptRecord::is_successful`, not `failure.is_none()`:
    /// the grid below is every combination of the two facts that predicate
    /// reads, and the line says "passed" exactly where the predicate says so.
    /// The pre-repair code answered "passed" for a record with no failure and
    /// a review that rejected it.
    #[test]
    fn an_attempt_is_described_as_passed_only_where_the_record_is_successful() {
        let failures = [None, Some(gate_failure("gate `test` failed: exit 1"))];
        let review_sets = [
            Vec::new(),
            vec![review("review", ReviewPassOutcome::Passed)],
            vec![
                review("review", ReviewPassOutcome::Passed),
                review("second-opinion", ReviewPassOutcome::Failed),
            ],
            vec![review("review", ReviewPassOutcome::Unavailable)],
        ];
        for failure in &failures {
            for reviews in &review_sets {
                let record = record(failure.clone(), reviews.clone());
                let successful = record.is_successful();
                let line = describe(&finished(record, None, None));
                assert_eq!(line.contains("t1: attempt 1 passed"), successful, "{line}");
            }
        }

        let rejected = describe(&finished(
            record(
                None,
                vec![
                    review("review", ReviewPassOutcome::Passed),
                    review("second-opinion", ReviewPassOutcome::Failed),
                ],
            ),
            None,
            None,
        ));
        assert!(
            rejected.contains(
                "t1: attempt 1 was not approved — review `second-opinion` (gpt-5) rejected it"
            ),
            "{rejected}"
        );
        let unavailable = describe(&finished(
            record(None, vec![review("review", ReviewPassOutcome::Unavailable)]),
            None,
            None,
        ));
        assert!(
            unavailable.contains(
                "t1: attempt 1 was not approved — review `review` (gpt-5) reached no verdict"
            ),
            "{unavailable}"
        );
    }

    /// The transition and the parking are two halves of one settlement, and
    /// the line renders each on its own. The pre-repair code rendered a parked
    /// attempt's transition only when it was an escalation, so a parking
    /// beside a `Fail` — a shape no writer produces today — dropped the task's
    /// failure from the line.
    #[test]
    fn a_parked_attempt_renders_every_transition_recorded_beside_the_parking() {
        let parked_failure = describe(&finished(
            record(Some(gate_failure("the attempt failed")), Vec::new()),
            Some(parking("q-1")),
            Some(AttemptTransition::Fail(TaskFailed {
                kind: FailureKind::GateFailed,
                reason: "the attempt failed".to_owned(),
                halts_run: true,
            })),
        ));
        assert!(
            parked_failure.contains(
                "t1: attempt 1 failed — the attempt failed; task failed (GateFailed) — \
                 the attempt failed; the run halts; parked on question q-1"
            ),
            "{parked_failure}"
        );

        // A parking with no failure and no transition says what the record
        // says, rather than inventing a "policy refusal" for it.
        let parked_pass = describe(&finished(
            record(None, Vec::new()),
            Some(parking("q-1")),
            None,
        ));
        assert!(
            parked_pass.contains("t1: attempt 1 passed; parked on question q-1"),
            "{parked_pass}"
        );
    }

    /// Every field on a line is on-disk data, and a failure reason quotes an
    /// agent's stderr. The pre-repair line carried the reason verbatim, so a
    /// newline in it split one event across lines of `--follow` and an escape
    /// sequence in it reached the terminal.
    #[test]
    fn describe_is_one_line_with_no_control_character_whatever_the_log_carries() {
        let reason =
            "agent error (exit Some(1)): first line\n\u{1b}[31msecond line\u{1b}[0m\r\n\tthird";
        let line = describe(&finished(
            record(Some(gate_failure(reason)), Vec::new()),
            None,
            None,
        ));
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(!line.contains('\u{1b}'), "{line:?}");
        assert!(
            line.contains("first line \\u{1b}[31msecond line\\u{1b}[0m   third"),
            "the control characters are shown, not passed through: {line:?}"
        );

        // Not only the reason: the guarantee is on the assembled line, so a
        // field the arm did not think of is covered too.
        let parked = describe(&event(EventBody::TaskParked {
            task: "t1".to_owned(),
            data: TaskParked {
                question: "q-1\u{1b}[2J\nq-2".to_owned(),
                refund_attempt: false,
            },
        }));
        assert_eq!(parked.lines().count(), 1, "{parked:?}");
        assert!(!parked.contains('\u{1b}'), "{parked:?}");
        assert!(parked.contains("parked on q-1\\u{1b}[2J q-2"), "{parked:?}");

        // And a line with nothing to change is the line as assembled.
        let plain = describe(&finished(record(None, Vec::new()), None, None));
        assert_eq!(plain, "14:03:07Z  t1: attempt 1 passed");
    }

    /// Abbreviation checks calendar dates, clock ranges, and the complete
    /// suffix. Leap-second records retain their date because this renderer
    /// does not consult the historical leap-second schedule. Event.ts is an
    /// unconstrained String and parsing validates nothing about it.
    #[test]
    fn a_timestamp_the_engine_did_not_write_is_printed_whole_rather_than_sliced() {
        let with = |ts: &str| Event {
            ts: ts.to_owned(),
            body: EventBody::TaskParked {
                task: "t1".to_owned(),
                data: TaskParked {
                    question: "q-1".to_owned(),
                    refund_attempt: false,
                },
            },
        };
        for whole in [
            "sometime",
            "xxxxxxxxxxx14:03:07Z",
            "2026-08-09 14:03:07Z",
            "2026/08/09T14:03:07Z",
            "2026-08-09T14.03.07Z",
            "2026-08-09T1a:03:07Z",
            "2026-08-09T14:03:0",
            "2026-00-09T14:03:07Z",
            "2026-13-09T14:03:07Z",
            "2026-08-00T14:03:07Z",
            "2026-08-32T14:03:07Z",
            "2026-04-31T14:03:07Z",
            "2026-02-29T14:03:07Z",
            "1900-02-29T14:03:07Z",
            "2100-02-29T14:03:07Z",
            "2000-02-30T14:03:07Z",
            "2026-08-09T24:03:07Z",
            "2026-08-09T14:60:07Z",
            "2026-08-09T14:03:61Z",
            "2016-12-31T23:59:60Z",
            "2026-08-09T14:03:07",
            "2026-08-09T14:03:07.Z",
            "2026-08-09T14:03:07.25",
            "2026-08-09T14:03:07+24:00",
            "2026-08-09T14:03:07-00:60",
            "2026-08-09T14:03:07+02:0",
            "2026-08-09T14:03:07+0200",
            "2026-08-09T14:03:07Zjunk",
            "2026-08-09T14:03:07+02:00junk",
            "2026-13-40T14:03:07+99:99junk",
            "2026-08-09T14:03:07.１２Z",
            "２０２６-08-09T14:03:07Z",
        ] {
            let line = describe(&with(whole));
            assert_eq!(line, format!("{whole}  t1: parked on q-1"), "{whole}");
        }
        for (timestamp, clock) in [
            ("2026-08-09T14:03:07+02:00", "14:03:07+02:00"),
            ("2026-08-09T14:03:07.250Z", "14:03:07.250Z"),
            ("2026-08-09T14:03:07.1-00:00", "14:03:07.1-00:00"),
            ("2026-08-09t14:03:07z", "14:03:07z"),
            ("2000-02-29T00:00:00Z", "00:00:00Z"),
            ("2024-02-29T23:59:59-23:59", "23:59:59-23:59"),
            ("0000-01-01T00:00:00+23:59", "00:00:00+23:59"),
            ("9999-12-31T23:59:59.123456789Z", "23:59:59.123456789Z"),
        ] {
            assert_eq!(
                describe(&with(timestamp)),
                format!("{clock}  t1: parked on q-1")
            );
        }
    }
}
