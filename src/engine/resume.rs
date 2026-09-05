//! Extended notes: `docs/internals/engine/resume.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::capacity;
use crate::config;
use crate::error::UpstrokeError;
use crate::events::{self, EventBody, EventLog, RunState, TaskState};
use crate::interaction::{self, RealSleeper};
use crate::ir::{Answer, Plan, QuestionId, ResolvedEffortPolicy};
use crate::ladder::FailureKind;
use crate::rundir::{self, RunLock, RunPaths, WorktreeLock};
use crate::runner::Runner;
use crate::util;
use crate::workspace::Workspace;

use super::coordinator::{Run, prepared_pin_ref};
use super::options::{Harness, ResumeOptions, RunOptions};
use super::preflight::{
    Preflight, Recorded, RecordedRouting, chain_summaries, normalized_plan_bytes,
    preflight_with_recorded, validate_inputs,
};
use super::report::{RunReport, last_reason};
use crate::topology::effects::EventSite;

#[cfg(test)]
pub(super) fn resume_harness_inner(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), UpstrokeError> {
    let contained = crate::runner::host::contain_write_command(&mut crate::agent::proc::NoHooks)?;
    resume_harness_inner_on(
        opts,
        harness,
        &crate::runner::host::HostRunner::new(),
        &contained,
    )
}

pub(super) fn resume_harness_inner_on(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    _contained: &crate::runner::host::Contained,
) -> Result<(RunReport, RunState), UpstrokeError> {
    let run_id = rundir::resolve_run_id(&opts.repo_root, &opts.run_id)?;
    let public = rundir::public_dir(&opts.repo_root, &run_id);
    let refuse = |message: String| UpstrokeError::Resume {
        run_id: run_id.clone(),
        message,
    };
    let events_path = public.join("events.jsonl");

    // This unlocked header read only selects the inputs for read-only refusal.
    // Resume adopts the authoritative log only after both leases are held below.
    let mut header_warnings = Vec::new();
    let header_events = events::read_all(&events_path, &mut header_warnings)?;
    let header = events::started_of(&header_events, &events_path)?.clone();
    let header_schema = events::ensure_supported_schema(&header, &header_events, &events_path)?;

    let mut run_opts = RunOptions::new(
        opts.repo_root.join(&header.plan_path),
        opts.repo_root.clone(),
    );
    run_opts.config_path = opts
        .config_path
        .clone()
        .or_else(|| header.config_path.as_ref().map(|p| opts.repo_root.join(p)));
    run_opts.pools_path = opts.pools_path.clone();
    run_opts.interaction = opts.interaction;
    run_opts.attempt_timeout = opts.attempt_timeout;
    run_opts.defer_backoff = opts.defer_backoff;
    run_opts.max_defers = opts.max_defers;
    run_opts.private_root = opts.private_root.clone();
    run_opts.wait_on_block = opts.wait_on_block;
    let wait_on_block = opts.wait_on_block;

    let limits = config::EngineLimits::for_resume(header_schema);
    let validated = validate_inputs(&run_opts, limits)?;

    let workspace = Workspace::open(&opts.repo_root)?;
    let worktree_git_dir = workspace.worktree_git_dir()?;
    // Acquire the physical worktree before the per-run lease, as a fresh run
    // does. Success grants this conductor ownership of the shared Git state;
    // a competing holder is refused before replay or branch mutation. Keep
    // both guards for the whole resume and release them in reverse order on
    // return or unwinding. The OS releases primary holds if the process dies;
    // the lock implementation also refuses while prior agents are cleaning up.
    let _worktree_lock = WorktreeLock::acquire_in(workspace.root(), &worktree_git_dir)?;

    // Claim the run before acting on its log, so two resumes cannot race into
    // the same branch. The lock is in the known public directory; the private
    // directory comes from the authoritative run_started read under this hold.
    let _lock = RunLock::acquire(&public)?;
    let _cleanup_scope = _lock.enter_cleanup_scope();

    let mut warnings = Vec::new();
    let events = events::read_all(&events_path, &mut warnings)?;
    let started = events::started_of(&events, &events_path)?.clone();
    let effective_schema = events::ensure_supported_schema(&started, &events, &events_path)?;
    if started.plan_path != header.plan_path || started.config_path != header.config_path {
        return Err(refuse(
            "this run's opening record changed while the resume was waiting for the worktree \
             lease: it now names a different plan or config. Preserve this log for recovery and \
             start a new run rather than continuing against a record that moved."
                .to_owned(),
        ));
    }
    let analysis = validated.confirm_under_lease(
        &run_opts,
        config::EngineLimits::for_resume(effective_schema),
    )?;
    let recorded_normalized_plan_digest =
        events::recorded_normalized_plan_digest(&events).map(str::to_owned);
    let frozen_plan_path = public.join("plan.normalized.json");
    let frozen_plan_bytes = fs::read(&frozen_plan_path).map_err(|source| UpstrokeError::Io {
        path: frozen_plan_path.clone(),
        source,
    })?;
    let frozen_plan_digest = events::normalized_plan_digest(&frozen_plan_bytes);
    if let Some(recorded) = recorded_normalized_plan_digest.as_deref() {
        if frozen_plan_digest != recorded {
            return Err(refuse(format!(
                "the exact bytes at {} no longer match this run's recorded normalized-plan digest ({recorded}, now {frozen_plan_digest}). Restore the frozen snapshot or start a new run.",
                frozen_plan_path.display()
            )));
        }
    }
    if let Some(failure) = events::legacy_unsettled_failure(started.schema, &events) {
        let detail = match failure.kind {
            events::LegacyUnsettledFailureKind::MissingDecision => {
                "without its durable ladder or parking decision"
            }
            events::LegacyUnsettledFailureKind::MissingSpendParking => {
                "after raising an ApproveSpend question but before durably parking the task"
            }
        };
        return Err(refuse(format!(
            "legacy event schema {} records failed attempt {} for `{}` on rung {} {detail}. The old writer may have stopped between two appends, so resuming could repeat paid work, choose the wrong rung, or bypass required spend approval. Preserve this log for recovery and start a new run rather than guessing.",
            started.schema, failure.attempt, failure.task, failure.rung,
        )));
    }
    let recorded_gates = events::recorded_gates(&events).cloned();
    let recorded_effort_policy = events::recorded_effort_policy(&events);
    let recorded_complete_reviews = events::recorded_complete_reviews(&events).cloned();
    let recorded_reviews = events::recorded_reviews(&events).cloned();
    let recorded_chains = events::recorded_chains(&events).cloned();

    let Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        warnings: preflight_warnings,
        mode,
        notifiers,
        budgets,
    } = preflight_with_recorded(
        &run_opts,
        harness,
        runner,
        analysis,
        Recorded {
            reviews: recorded_reviews.clone(),
            gates: recorded_gates.clone(),
            legacy_review_timeout_missing: recorded_reviews
                .as_ref()
                .is_some_and(|plan| plan.pass_timeout_secs.is_none()),
            gates_from_config: started.gates_from_config,
            routing: Some(RecordedRouting {
                run_id: run_id.clone(),
                structure: started.chains.clone(),
                bindings: recorded_chains.clone(),
            }),
        },
    )?;
    if recorded_reviews.is_none() {
        warnings.push(
            "this run's log predates the review record (step 9), so who reviews was re-derived \
             from today's config rather than read from the run — earlier tasks may have been \
             judged differently"
                .to_owned(),
        );
    }
    if recorded_gates.is_none() {
        let names_now: Vec<String> = gates.iter().map(|gate| gate.name.clone()).collect();
        if names_now != started.gates {
            warnings.push(format!(
                "this run's log predates the gate record, so its gates were re-derived from \
                 today's config — and the gate names have moved, so the tasks it already \
                 committed were verified differently: it recorded [{}], today resolves [{}]",
                render_names(&started.gates),
                render_names(&names_now),
            ));
        } else if !names_now.is_empty() {
            warnings.push(format!(
                "this run's log predates the gate record, so its gates were re-derived from \
                 today's config rather than rebuilt from the run. The names still match what it \
                 recorded ([{}]), but a log this old cannot show whether a command behind one of \
                 them changed",
                render_names(&names_now),
            ));
        }
    }
    let current_effort_policy = analysis.config.resolved_effort_policy();
    let effort_policy = recorded_effort_policy.unwrap_or(current_effort_policy);
    match recorded_effort_policy {
        None => warnings.push(
            "this run's log predates the effort-policy record, so implementation and review \
             effort were re-derived from today's config rather than read from the run — earlier \
             attempts may have used a different effort standard"
                .to_owned(),
        ),
        Some(recorded) if recorded != current_effort_policy => warnings.push(format!(
            "today's effort policy ({}) differs from the one this run recorded ({}). This \
             resume keeps the recorded policy so one run has one execution and review standard. \
             Start a new run to adopt today's policy.",
            render_effort_policy(current_effort_policy),
            render_effort_policy(recorded),
        )),
        Some(_) => {}
    }
    warnings.extend(preflight_warnings);

    if analysis.plan.source.hash != started.plan_hash {
        return Err(refuse(format!(
            "the plan at {} has changed since this run froze it (recorded {}, now {}). Task \
             progress is recorded per task, so replaying it against a different plan would \
             attribute work to the wrong tasks. Restore the plan, or start a new run.",
            run_opts.plan_path.display(),
            started.plan_hash,
            analysis.plan.source.hash
        )));
    }
    let canonical_plan_bytes = normalized_plan_bytes(&analysis.plan, &frozen_plan_path)?;
    let canonical_plan_digest = events::normalized_plan_digest(&canonical_plan_bytes);
    let established_normalized_plan_digest = if let Some(recorded) =
        recorded_normalized_plan_digest.as_deref()
    {
        if canonical_plan_digest != recorded {
            return Err(refuse(format!(
                "the validated source plan now normalizes to digest {canonical_plan_digest}, but this run recorded {recorded}. Restore the source plan semantics or start a new run."
            )));
        }
        None
    } else {
        if canonical_plan_bytes != frozen_plan_bytes {
            return Err(refuse(format!(
                "legacy frozen plan {} does not exactly match the canonical serialization of the validated source plan. Refusing to bless a mutable legacy snapshot during the schema-3 upgrade; restore it or start a new run.",
                frozen_plan_path.display()
            )));
        }
        Some(frozen_plan_digest.clone())
    };

    let task_ids: Vec<String> = analysis
        .plan
        .tasks
        .iter()
        .map(|task| task.id.to_string())
        .collect();
    let replayed = events::replay(events, task_ids, &events_path)?;

    match replayed.state.finished.as_ref().map(|f| &f.outcome) {
        Some(events::RunOutcome::Complete) => {
            return Err(refuse(
                "this run already completed; there is nothing left to continue".to_owned(),
            ));
        }
        Some(events::RunOutcome::Halted) => {
            return Err(refuse(format!(
                "this run halted at `{}` under `on_task_failure = \"halt\"`. Nothing can run \
                 while it is halted — fix what failed and start a new run.",
                replayed.state.halted_at.as_deref().unwrap_or("?")
            )));
        }
        Some(events::RunOutcome::Parked | events::RunOutcome::BudgetExceeded) | None => {}
    }

    let defect_questions: BTreeSet<QuestionId> = replayed
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::DesignDefect { data } => Some(data.question.clone()),
            _ => None,
        })
        .collect();
    let missing_answer_defects: Vec<_> = replayed
        .state
        .questions
        .iter()
        .filter_map(|record| {
            let answer = record.answer.as_ref()?;
            (!defect_questions.contains(&record.question.id)).then(|| {
                (
                    record.question.id.clone(),
                    util::head(record.question.context.trim(), 600),
                    match answer {
                        Answer::Answered { text } => text.clone(),
                        _ => "declined".to_owned(),
                    },
                )
            })
        })
        .collect();
    let decline_halt_policies: BTreeMap<_, _> = replayed
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::QuestionAnswered { data } if data.answer == Answer::Declined => {
                Some((data.question.clone(), data.decline_halts_run))
            }
            _ => None,
        })
        .collect();
    let mut declined_questions = Vec::new();
    for record in replayed
        .state
        .questions
        .iter()
        .filter(|record| record.answer.as_ref() == Some(&Answer::Declined))
    {
        let affected: Vec<_> = record
            .question
            .affected_tasks
            .iter()
            .filter(|task_id| {
                replayed
                    .state
                    .index_of(task_id.as_str())
                    .is_some_and(|index| {
                        matches!(
                            &replayed.state.states[index],
                            TaskState::AwaitingInput(open) if open == &record.question.id
                        )
                    })
            })
            .cloned()
            .collect();
        if affected.is_empty() {
            continue;
        }
        let Some(halts_run) = decline_halt_policies
            .get(&record.question.id)
            .copied()
            .flatten()
        else {
            return Err(refuse(format!(
                "legacy declined answer {} stopped before settling its affected task, but the log does not record the contemporaneous on_task_failure policy. Today's config cannot safely decide an old answer; preserve this log for recovery and start a new run.",
                record.question.id
            )));
        };
        declined_questions.push((record.question.id.clone(), affected, halts_run));
    }

    let paths = match &opts.private_root {
        Some(root) => RunPaths::with_private_root(&opts.repo_root, &run_id, root),
        None => RunPaths::from_parts(public.clone(), PathBuf::from(&started.private_dir)),
    };
    paths.create()?;

    let reclaimed = workspace.reclaim_gate_workspaces(&paths.gate_worktrees())?;
    if reclaimed > 0 {
        warnings.push(format!(
            "reclaimed {reclaimed} gate/review snapshot worktree(s) left by the interrupted run"
        ));
    }
    workspace.ensure_execution_prerequisites()?;
    workspace.ensure_run_exclusions()?;
    if !workspace.branch_exists(&started.branch)? {
        return Err(refuse(format!(
            "the run branch `{}` no longer exists. Its commits are what this run's record \
             refers to; without it there is nothing to continue onto.",
            started.branch
        )));
    }
    if workspace.current_branch()? != started.branch {
        if !workspace.is_clean()? {
            return Err(refuse(format!(
                "you have uncommitted changes and are not on `{}`. Commit or stash them, then \
                 resume — switching branches over them would lose work that is not this run's \
                 to discard.",
                started.branch
            )));
        }
        workspace.switch_branch(&started.branch)?;
    }

    let recorded_head = last_committed_sha(&replayed.events).unwrap_or(started.base_sha.clone());
    let mut head = workspace.head_sha_full()?;

    let mut adopted = None;
    if let Some((task, message, prepared)) = unrecorded_commit(&replayed, &analysis.plan) {
        let Some(prepared) = prepared else {
            if head != recorded_head {
                return Err(refuse(format!(
                    "`{}` is at {head}, but the successful legacy settlement for `{task}` did \
                     not record an exact prepared commit. Refusing to adopt a commit by subject \
                     alone; move the branch back to {recorded_head}, or start a new run.",
                    started.branch
                )));
            }
            return Err(refuse(format!(
                "the successful legacy settlement for `{task}` has no exact prepared commit. \
                 It cannot be replayed safely; preserve this log for recovery and start a new run."
            )));
        };
        if prepared.parent_sha != recorded_head
            || prepared.message != message
            || !workspace.prepared_commit_matches(&prepared)?
        {
            return Err(refuse(format!(
                "the recorded prepared commit for `{task}` does not match its task, parent, or \
                 Git object. Refusing to publish or adopt it; preserve the log for recovery."
            )));
        }
        let observed_branch_ref = workspace.current_branch_ref()?;
        if observed_branch_ref != prepared.branch_ref {
            return Err(refuse(format!(
                "HEAD is on `{observed_branch_ref}`, not the prepared commit's recorded branch \
                 `{}`; refusing prepared recovery.",
                prepared.branch_ref
            )));
        }

        if head == prepared.parent_sha {
            if workspace.prepared_pin_target(&prepared.pin_ref)?.as_deref()
                != Some(prepared.commit_sha.as_str())
            {
                return Err(refuse(format!(
                    "the recorded prepared commit for `{task}` is not pinned by `{}`. Refusing \
                     to publish an unprotected or substituted object; preserve the log for recovery.",
                    prepared.pin_ref
                )));
            }
            workspace.advance_prepared_commit(&prepared.branch_ref, &prepared)?;
            head = prepared.commit_sha.clone();
            warnings.push(format!(
                "published prepared commit {head} for `{task}` after the run stopped between \
                 settlement and the branch update"
            ));
            adopted = Some((task, message));
        } else if head == prepared.commit_sha {
            match workspace.prepared_pin_target(&prepared.pin_ref)? {
                Some(target) if target == prepared.commit_sha => {
                    workspace.remove_prepared_pin(&prepared)?;
                }
                Some(target) => {
                    return Err(refuse(format!(
                        "prepared ref `{}` points at {target}, not the recorded commit {}; \
                         refusing to delete or adopt a substituted object.",
                        prepared.pin_ref, prepared.commit_sha
                    )));
                }
                None => {}
            }
            warnings.push(format!(
                "adopted commit {head} as `{task}` from its exact prepared identity after the \
                 run stopped before recording it"
            ));
            adopted = Some((task, message));
        }
    }

    if adopted.is_none() && head != recorded_head {
        return Err(refuse(format!(
            "`{}` is at {head}, but this run's record ends at {recorded_head}. Something \
             committed, reset, or rebased the branch after the run stopped, so replaying the \
             log would describe work that is no longer what is on the branch. Move the branch \
             back to {recorded_head}, or start a new run.",
            started.branch
        )));
    }

    for interrupted in replayed.state.interrupted_attempts() {
        #[expect(
            clippy::expect_used,
            reason = "interrupted_attempts() yields tasks of this same replayed state"
        )]
        let task_index = replayed
            .state
            .index_of(&interrupted.task)
            .expect("an interrupted task belongs to the replayed plan");
        let pin_ref = prepared_pin_ref(&run_id, task_index, interrupted.flight.attempt);
        if workspace.prepared_pin_target(&pin_ref)?.is_some() {
            workspace.remove_orphan_prepared_pin(&pin_ref)?;
            warnings.push(format!(
                "removed orphan prepared commit pin `{pin_ref}` for interrupted attempt {}",
                interrupted.flight.attempt
            ));
        }
    }

    let discarded = workspace.uncommitted_summary()?;
    if !discarded.is_empty() {
        warnings.push(format!(
            "discarded {} uncommitted path(s) left by the interrupted run: {}",
            discarded.len(),
            discarded.join(", ")
        ));
        workspace.discard_uncommitted()?;
    }

    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    let prior_signals = capacity::observe(&replayed.events).exhausted;
    let log = EventLog::open(EventSite::LegacyOpenLog, &paths.events(), &mut warnings)?;
    let established_reviews = recorded_complete_reviews
        .is_none()
        .then(|| review_plan.clone());
    let mut run = Run {
        state: replayed.state,
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        log_hooks: Box::new(crate::events::log::NoEventHooks),
        gate_cmds,
        adapters: harness.adapters,
        runner,
        answers: harness.answers.unwrap_or(default_answers.as_ref()),
        notifiers,
        sleeper,
        caps,
        review_plan,
        effort_policy,
        attempt_timeout: opts.attempt_timeout,
        review_pass_timeout,
        defer_backoff: opts.defer_backoff,
        max_defers: opts.max_defers,
        on_task_failure: analysis.config.on_task_failure,
        budgets,
        ask_before: analysis.config.ask_before,
        run_id,
        branch: started.branch.clone(),
        warnings,
        unanswerable: Vec::new(),
        exhausted_pools: prior_signals.keys().cloned().collect(),
        #[cfg(test)]
        after_candidate_capture: None,
    };
    if let Some((task, message)) = adopted {
        run.emit(EventBody::TaskCommitted {
            task,
            data: events::TaskCommitted {
                sha: head.clone(),
                message,
            },
        })?;
    }
    if effective_schema < events::SCHEMA_VERSION {
        run.emit(EventBody::RunSchemaUpgraded {
            data: events::RunSchemaUpgraded {
                from: effective_schema,
                to: events::SCHEMA_VERSION,
            },
        })?;
    }
    for (question, context, answer) in missing_answer_defects {
        run.emit(EventBody::DesignDefect {
            data: events::DesignDefect {
                question,
                context,
                answer,
            },
        })?;
    }
    for (question, affected, halts_run) in declined_questions {
        for task_id in affected {
            let Some(index) = run.state.index_of(task_id.as_str()) else {
                continue;
            };
            if !matches!(&run.state.states[index], TaskState::AwaitingInput(open) if open == &question)
            {
                continue;
            }
            let reason = format!(
                "declined at the human rung: {}",
                last_reason(&run.state.progress[index])
            );
            run.fail_task_with_policy(index, FailureKind::Declined, reason, halts_run)?;
        }
    }
    for record in &run.state.questions {
        interaction::write_question(&run.paths.questions(), record)?;
    }

    let interrupted = run.state.interrupted_attempts();
    for attempt in &interrupted {
        run.emit(attempt.event())?;
    }

    run.emit(EventBody::RunResumed {
        data: events::RunResumed {
            head_sha: head,
            interrupted_attempts: u32::try_from(interrupted.len()).unwrap_or(u32::MAX),
            discarded,
            gates: recorded_gates.is_none().then(|| gates.clone()),
            effort_policy: recorded_effort_policy.is_none().then_some(effort_policy),
            reviews: established_reviews,
            chains: recorded_chains
                .is_none()
                .then(|| chain_summaries(&analysis)),
            normalized_plan_digest: established_normalized_plan_digest,
        },
    })?;
    run.emit_capacity_snapshot(&prior_signals)?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

fn render_names(names: &[String]) -> String {
    names.join(", ")
}

fn render_effort_policy(policy: ResolvedEffortPolicy) -> String {
    format!(
        "implementation small={}, mid={}, frontier={}; review={}",
        policy.small, policy.mid, policy.frontier, policy.review
    )
}

fn last_committed_sha(events: &[events::Event]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.body {
        EventBody::TaskCommitted { data, .. } => Some(data.sha.clone()),
        _ => None,
    })
}

fn unrecorded_commit(
    replayed: &events::Replay,
    plan: &Plan,
) -> Option<(String, String, Option<events::PreparedCommit>)> {
    let EventBody::AttemptFinished {
        task,
        data,
        prepared_commit,
        ..
    } = &replayed.events.last()?.body
    else {
        return None;
    };
    if data.failure.is_some() {
        return None;
    }
    let index = replayed.state.index_of(task)?;
    if replayed.state.states[index] != TaskState::Pending {
        return None;
    }
    let task = plan.tasks.get(index)?;
    Some((
        task.id.to_string(),
        format!("[upstroke] {}: {}", task.id, task.title),
        prepared_commit.as_deref().cloned(),
    ))
}
