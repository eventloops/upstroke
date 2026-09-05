//! Extended notes: `docs/internals/engine/coordinator.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::time::Duration;

use crate::agent::{AdapterSource, Caps};
use crate::capacity;
use crate::config::{self, OnTaskFailure};
use crate::error::UpstrokeError;
use crate::events::{self, EventBody, EventLog, Progress, RunState, TaskState};
use crate::interaction::{self, AnswerSource, Notifier, QuestionRecord, RealSleeper, Sleeper};
use crate::ir::{
    Answer, Question, QuestionId, QuestionKind, ResolvedEffortPolicy, Task, WorkerProfile,
};
use crate::ladder::{
    self, AttemptFailure, FailureKind, FailureOrigin, LadderPolicy, LadderState, Next,
};
use crate::review::{PassBinding, ReviewPass, ReviewPlan};
use crate::rundir::{self, RunLock, RunPaths, WorktreeLock};
use crate::runner::Runner;
use crate::topology::effects::EventSite;
use crate::ulid;
use crate::util;
use crate::validate::Analysis;
use crate::workspace::Workspace;

use super::attempt::{AttemptCx, RetryBrief, Reviewer, pool_option, run_attempt};
#[cfg(test)]
use super::options::AfterCandidateCapture;
use super::options::{Harness, RunOptions};
use super::preflight::{
    Preflight, chain_summaries, normalized_plan_bytes, preflight, repo_relative, validate_inputs,
};
use super::report::{
    ReportHeader, RunOutcome, RunReport, TaskRunStatus, build_report, last_reason,
};

#[cfg(test)]
pub(super) fn run_harness_inner(
    opts: &RunOptions,
    harness: &Harness<'_>,
) -> Result<(RunReport, RunState), UpstrokeError> {
    let contained = crate::runner::host::contain_write_command(&mut crate::agent::proc::NoHooks)?;
    run_harness_inner_on(
        opts,
        harness,
        &crate::runner::host::HostRunner::new(),
        &contained,
    )
}

pub(super) fn run_harness_inner_on(
    opts: &RunOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    _contained: &crate::runner::host::Contained,
) -> Result<(RunReport, RunState), UpstrokeError> {
    let validated = validate_inputs(opts, config::EngineLimits::Fresh)?;
    let workspace = Workspace::open(&opts.repo_root)?;
    let worktree_git_dir = workspace.worktree_git_dir()?;
    let _worktree_lock = WorktreeLock::acquire_in(workspace.root(), &worktree_git_dir)?;
    let analysis = validated.confirm_under_lease(opts, config::EngineLimits::Fresh)?;
    let Preflight {
        analysis,
        caps,
        review_plan,
        review_pass_timeout,
        gates,
        gate_cmds,
        mut warnings,
        mode,
        notifiers,
        budgets,
    } = preflight(opts, harness, runner, analysis)?;

    workspace.ensure_execution_prerequisites()?;
    workspace.ensure_run_exclusions()?;
    if !workspace.is_clean()? {
        return Err(UpstrokeError::Git {
            message: "working tree is not clean; commit or stash first (the engine refuses \
                      dirty trees)"
                .to_owned(),
        });
    }
    let base_sha = workspace.head_sha_full()?;
    let wait_on_block = opts.wait_on_block;

    let run_id = ulid::ulid();
    let branch = format!("upstroke/run-{run_id}");
    let paths = opts.paths(&run_id);
    paths.create()?;
    let _lock = RunLock::acquire(&paths.public)?;
    let _cleanup_scope = _lock.enter_cleanup_scope();

    let plan_path = paths.plan_json();
    let normalized_plan = normalized_plan_bytes(&analysis.plan, &plan_path)?;
    let normalized_plan_digest = events::normalized_plan_digest(&normalized_plan);
    let opened = rundir::write_plan(&paths.public, &normalized_plan, &mut rundir::NoHooks)
        .and_then(|()| {
            let read_back = fs::read(&plan_path).map_err(|source| UpstrokeError::Io {
                path: plan_path.clone(),
                source,
            })?;
            if read_back != normalized_plan {
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "{} changed while upstroke was freezing it; refusing to record a digest for bytes it did not write",
                        plan_path.display()
                    ),
                });
            }
            workspace.create_branch(&branch)
        });
    if let Err(error) = opened {
        drop(_cleanup_scope);
        drop(_lock);
        let _ = fs::remove_dir_all(&paths.public);
        let _ = fs::remove_dir_all(&paths.private);
        return Err(error);
    }

    let effort_policy = analysis.config.resolved_effort_policy();
    let started = events::RunStarted {
        schema: events::SCHEMA_VERSION,
        upstroke_version: env!("CARGO_PKG_VERSION").to_owned(),
        run_id: run_id.clone(),
        branch: branch.clone(),
        base_sha,
        plan_path: repo_relative(&opts.repo_root, &opts.plan_path),
        config_path: opts
            .config_path
            .as_ref()
            .map(|path| repo_relative(&opts.repo_root, path)),
        plan_hash: analysis.plan.source.hash.clone(),
        normalized_plan_digest: Some(normalized_plan_digest),
        private_dir: paths.private.to_string_lossy().into_owned(),
        gates: gates.iter().map(|gate| gate.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        interaction_mode: mode.to_string(),
        chains: chain_summaries(&analysis),
        effort_policy: Some(effort_policy),
        reviews: Some(review_plan.clone()),
        gate_cmds: Some(gates),
    };

    let sleeper = harness.sleeper.unwrap_or(&RealSleeper);
    let default_answers = interaction::answers_for(
        mode,
        paths.answers(),
        wait_on_block.unwrap_or(analysis.config.wait_on_block),
        sleeper,
    );
    let log = EventLog::open(EventSite::LegacyOpenLog, &paths.events(), &mut warnings)?;
    let mut run = Run {
        state: RunState::new(
            analysis
                .plan
                .tasks
                .iter()
                .map(|task| task.id.to_string())
                .collect(),
        ),
        analysis: &analysis,
        workspace: &workspace,
        paths,
        log,
        log_hooks: legacy_append_hooks(opts),
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
        branch,
        warnings,
        unanswerable: Vec::new(),
        exhausted_pools: std::collections::BTreeSet::new(),
        #[cfg(test)]
        after_candidate_capture: opts.after_candidate_capture,
    };
    run.emit(EventBody::RunStarted {
        data: Box::new(started),
    })?;
    run.emit_capacity_snapshot(&BTreeMap::new())?;
    let report = run.drain_and_report()?;
    Ok((report, run.state.clone()))
}

pub(super) fn prepared_pin_ref(run_id: &str, task_index: usize, attempt: u32) -> String {
    format!("refs/upstroke/prepared/{run_id}/{task_index}-{attempt}")
}

pub(super) struct Run<'a> {
    pub(super) analysis: &'a Analysis,
    pub(super) workspace: &'a Workspace,
    pub(super) paths: RunPaths,
    pub(super) log: EventLog,
    pub(super) log_hooks: Box<dyn crate::events::log::EventHooks>,
    pub(super) state: RunState,
    pub(super) gate_cmds: Vec<String>,
    pub(super) adapters: &'a dyn AdapterSource,
    pub(super) runner: &'a dyn Runner,
    pub(super) answers: &'a dyn AnswerSource,
    pub(super) notifiers: Vec<&'static dyn Notifier>,
    pub(super) sleeper: &'a dyn Sleeper,
    pub(super) caps: BTreeMap<String, Caps>,
    pub(super) review_plan: ReviewPlan,
    pub(super) effort_policy: ResolvedEffortPolicy,
    pub(super) attempt_timeout: Duration,
    pub(super) review_pass_timeout: Duration,
    pub(super) defer_backoff: Duration,
    pub(super) max_defers: u32,
    pub(super) on_task_failure: OnTaskFailure,
    pub(super) budgets: config::Budgets,
    pub(super) ask_before: config::AskBefore,
    pub(super) run_id: String,
    pub(super) branch: String,
    pub(super) warnings: Vec<String>,
    pub(super) unanswerable: Vec<QuestionId>,
    pub(super) exhausted_pools: std::collections::BTreeSet<String>,
    #[cfg(test)]
    pub(super) after_candidate_capture: Option<AfterCandidateCapture>,
}

#[cfg(test)]
fn legacy_append_hooks(opts: &RunOptions) -> Box<dyn crate::events::log::EventHooks> {
    match opts.log_hooks {
        Some(make) => make(),
        None => Box::new(crate::events::log::NoEventHooks),
    }
}

#[cfg(not(test))]
fn legacy_append_hooks(_opts: &RunOptions) -> Box<dyn crate::events::log::EventHooks> {
    Box::new(crate::events::log::NoEventHooks)
}

impl Run<'_> {
    pub(super) fn emit(&mut self, body: EventBody) -> Result<(), UpstrokeError> {
        let site = EventSite::LegacyAppend;
        let event = self
            .log
            .append_hooked(site, body, self.log_hooks.as_mut())?;
        self.state.apply(&event);
        Ok(())
    }

    pub(super) fn drain_and_report(&mut self) -> Result<RunReport, UpstrokeError> {
        if let Err(error) = self.drain() {
            let partial = self.finish();
            let _ = rundir::write_report(&self.paths.public, &partial, &mut rundir::NoHooks);
            return Err(error);
        }
        let report = self.finish();
        let committed = report
            .tasks
            .iter()
            .filter(|task| matches!(task.status, TaskRunStatus::Committed { .. }))
            .count();
        self.emit(EventBody::RunFinished {
            data: events::RunFinished {
                outcome: match report.outcome() {
                    RunOutcome::Complete => events::RunOutcome::Complete,
                    RunOutcome::Parked => events::RunOutcome::Parked,
                    RunOutcome::Halted => events::RunOutcome::Halted,
                    RunOutcome::BudgetExceeded => events::RunOutcome::BudgetExceeded,
                },
                halted_at: report.halted_at.clone(),
                committed: u32::try_from(committed).unwrap_or(u32::MAX),
                parked: u32::try_from(report.parked_tasks().len()).unwrap_or(u32::MAX),
            },
        })?;
        rundir::write_report(&self.paths.public, &report, &mut rundir::NoHooks)?;
        Ok(report)
    }

    fn drain(&mut self) -> Result<(), UpstrokeError> {
        let mut defer_round = 0u32;
        loop {
            if self.state.budget_stop.is_none() && self.sweep_answers()? {
                continue;
            }
            if let Some(index) = self.next_ready() {
                let deferred = self.step_task(index)?;
                if !deferred {
                    defer_round = 0;
                }
                continue;
            }
            if self.state.states.contains(&TaskState::Deferred)
                && self.state.halted_at.is_none()
                && self.state.budget_stop.is_none()
            {
                let waited = interaction::defer_backoff(self.defer_backoff, defer_round);
                self.sleeper.sleep(waited);
                defer_round = defer_round.saturating_add(1);
                self.emit(EventBody::DeferWaitElapsed {
                    data: events::DeferWaitElapsed {
                        waited,
                        round: defer_round,
                    },
                })?;
                continue;
            }
            if self.state.halted_at.is_none()
                && self.state.budget_stop.is_none()
                && self.resolve_one_question()?
            {
                continue;
            }
            break;
        }
        Ok(())
    }

    fn next_ready(&self) -> Option<usize> {
        if self.state.halted_at.is_some() || self.state.budget_stop.is_some() {
            return None;
        }
        let tasks = &self.analysis.plan.tasks;
        (0..tasks.len()).find(|&i| {
            matches!(self.state.states[i], TaskState::Pending)
                && tasks[i].depends_on.iter().all(|dep| {
                    tasks
                        .iter()
                        .position(|t| t.id == *dep)
                        .is_none_or(|j| matches!(self.state.states[j], TaskState::Done(_)))
                })
        })
    }

    fn step_task(&mut self, index: usize) -> Result<bool, UpstrokeError> {
        let analysis = self.analysis;
        let adapters = self.adapters;
        let workspace = self.workspace;
        let task = &analysis.plan.tasks[index];
        let task_id = task.id.to_string();
        let chain = &analysis.chains[index];
        let policy = LadderPolicy {
            attempts_per: chain.attempts_per,
            rungs: chain.rungs.len(),
            max_defers: self.max_defers,
        };
        let stem = format!("{index:02}-{}", util::filename_component(task.id.as_str()));

        loop {
            let rung_index = self.state.progress[index].rung;
            let Some(rung) = chain.rungs.get(rung_index) else {
                self.fail_task(
                    index,
                    FailureKind::NoChain,
                    "resolved chain has no rung to run on".to_owned(),
                )?;
                return Ok(false);
            };
            if let Some(exceeded) = self.budget_breach(index) {
                self.emit(EventBody::BudgetExceeded { data: exceeded })?;
                if let Err(error) = workspace.discard_uncommitted() {
                    self.warnings.push(format!(
                        "the budget stopped the run, but the working tree could not be cleaned: \
                         {error}"
                    ));
                }
                return Ok(false);
            }

            let profile = super::assembly::implementer_profile(
                super::assembly::ImplementerBinding::of_rung(
                    rung,
                    self.effort_policy.implementation_for(rung.tier),
                ),
                self.pool_name_for(&rung.binding.agent),
            );
            let adapter = adapters
                .get(&profile.agent)
                .ok_or_else(|| UpstrokeError::Agent {
                    message: format!("no adapter registered for agent `{}`", profile.agent),
                })?;

            let attempt = self.state.progress[index].attempts + 1;
            let resume = self.state.progress[index]
                .resume_next
                .then(|| self.state.progress[index].session.clone())
                .flatten();

            let rung_number = u32::try_from(rung_index).unwrap_or(u32::MAX);
            self.emit(EventBody::AttemptStarted {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                data: events::AttemptStarted {
                    tier: rung.tier.to_string(),
                    agent: profile.agent.clone(),
                    model: profile.model.clone(),
                    adapter: Some(adapter.id().to_owned()),
                    preflight_cli_version: self
                        .caps
                        .get(&profile.agent)
                        .map(|caps| caps.version.clone()),
                    effort: profile.effort,
                    selection_origin: Some(if rung.binding.pinned {
                        events::SelectionOrigin::Pin
                    } else {
                        events::SelectionOrigin::Auto
                    }),
                    pool: pool_option(&profile.pool),
                    resume_session: resume.clone(),
                },
            })?;

            let result = {
                let retry = (attempt > 1).then(|| RetryBrief {
                    resumed: resume.is_some(),
                    feedback: self.state.progress[index].feedback.clone(),
                });
                let attempt_cx = AttemptCx {
                    task,
                    profile: profile.clone(),
                    adapter,
                    runner: self.runner,
                    task_index: u32::try_from(index).unwrap_or(u32::MAX),
                    attempt,
                    stem: stem.clone(),
                    paths: &self.paths,
                    gates: &analysis.gates,
                    gate_cmds: &self.gate_cmds,
                    reviewers: self.reviewers(index, &profile)?,
                    timeout: self.attempt_timeout,
                    review_pass_timeout: self.review_pass_timeout,
                    retry,
                    decisions: self.state.progress[index]
                        .feedback
                        .iter()
                        .filter(|entry| entry.human)
                        .filter_map(|entry| entry.detail.clone())
                        .collect(),
                    #[cfg(test)]
                    after_candidate_capture: self.after_candidate_capture,
                };

                match run_attempt(&attempt_cx, workspace, resume.clone()) {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = workspace.discard_uncommitted();
                        return Err(error);
                    }
                }
            };

            let next = result.failure.as_ref().map(|failure| {
                let settlement_session = result.outcome.session_id.as_ref().or(resume.as_ref());
                let resumable = settlement_session.is_some()
                    && self
                        .caps
                        .get(&profile.agent)
                        .is_some_and(|c| c.session_resume);
                ladder::next_step(
                    failure,
                    &LadderState {
                        rung: self.state.progress[index].rung,
                        attempts_on_rung: self.state.progress[index].attempts_on_rung,
                        defers: self.state.progress[index].defers,
                        resumable,
                    },
                    &policy,
                )
            });
            let mut transition = None;
            let mut parking = None;
            let mut parking_question = None;
            let pending_spend = result.outcome.cost_usd.unwrap_or(0.0)
                + result
                    .reviews
                    .iter()
                    .map(|review| review.cost_usd.unwrap_or(0.0))
                    .sum::<f64>();
            let pending_unpriced = result.outcome.cost_usd.is_none()
                || result
                    .reviews
                    .iter()
                    .any(|review| review.cost_usd.is_none());
            if let (Some(failure), Some(next)) = (result.failure.as_ref(), next) {
                match next {
                    Next::RetrySameRung { resume } => {
                        transition = Some(Box::new(events::AttemptTransition::Retry(
                            events::LadderRetry {
                                resume,
                                tier: rung.tier.to_string(),
                                summary: failure.reason.clone(),
                                detail: failure.feedback.clone(),
                            },
                        )));
                    }
                    Next::Escalate => {
                        transition = Some(Box::new(events::AttemptTransition::Escalate(
                            events::LadderEscalated {
                                to_rung: rung_number.saturating_add(1),
                                tier: rung.tier.to_string(),
                                summary: failure.reason.clone(),
                                detail: failure.feedback.clone(),
                            },
                        )));
                        if let Some(onto) = chain.rungs.get(rung_index + 1).map(|next| next.tier) {
                            if self.should_approve_spend(rung.tier, onto, pending_spend) {
                                let question = self.build_spend_approval(
                                    index,
                                    onto,
                                    pending_spend,
                                    pending_unpriced,
                                );
                                parking = Some(Box::new(events::AttemptParking {
                                    question: question.clone(),
                                    refund_attempt: false,
                                }));
                                parking_question = Some(question);
                            }
                        }
                    }
                    Next::Defer => {
                        transition = Some(Box::new(events::AttemptTransition::Defer(
                            events::TaskDeferred {
                                reason: failure.reason.clone(),
                                defers: self.state.progress[index].defers.saturating_add(1),
                            },
                        )));
                    }
                    Next::AskHuman(kind) => {
                        let context = question_context(
                            ParkSubject::of(task, &self.state.progress[index]),
                            kind,
                            failure,
                        );
                        let question = self.build_question(index, kind, context);
                        parking = Some(Box::new(events::AttemptParking {
                            question: question.clone(),
                            refund_attempt: kind == QuestionKind::Clarify || failure.is_outage(),
                        }));
                        parking_question = Some(question);
                    }
                    Next::Fail => {
                        transition = Some(Box::new(events::AttemptTransition::Fail(
                            events::TaskFailed {
                                kind: failure.kind,
                                reason: failure.reason.clone(),
                                halts_run: self.on_task_failure == OnTaskFailure::Halt,
                            },
                        )));
                    }
                }
            }

            let prepared_commit = if result.failure.is_none() {
                let message = format!("[upstroke] {}: {}", task.id, task.title);
                let pin_ref = prepared_pin_ref(&self.run_id, index, attempt);
                let recorded_branch_ref = format!("refs/heads/{}", self.branch);
                if result.candidate_branch_ref != recorded_branch_ref {
                    let _ = self.workspace.discard_uncommitted();
                    return Err(UpstrokeError::Git {
                        message: format!(
                            "candidate was captured from `{}`, not recorded run branch `{recorded_branch_ref}`; refusing publication",
                            result.candidate_branch_ref
                        ),
                    });
                }
                match self.workspace.prepare_commit_from_candidate(
                    &result.candidate_branch_ref,
                    &result.candidate_parent,
                    &result.candidate_tree,
                    &message,
                    &pin_ref,
                ) {
                    Ok(prepared) => Some(prepared),
                    Err(error) => {
                        let _ = self.workspace.discard_uncommitted();
                        return Err(error);
                    }
                }
            } else {
                None
            };

            let settlement = self.emit(EventBody::AttemptFinished {
                task: task_id.clone(),
                attempt,
                rung: rung_number,
                profile: profile.name.clone(),
                parking,
                transition,
                prepared_commit: prepared_commit.clone().map(Box::new),
                data: Box::new(super::classify::attempt_record(
                    attempt,
                    super::classify::AttemptFacts {
                        tier: rung.tier,
                        model: &profile.model,
                        pool: pool_option(&profile.pool),
                        resumed: resume.is_some(),
                        outcome: &result.outcome,
                        reviews: &result.reviews,
                        failure: result.failure.as_ref(),
                        feedback: super::classify::FeedbackCarrier::LadderEvent,
                    },
                )),
            });
            if let Err(error) = settlement {
                if let Err(cleanup) = self.workspace.discard_uncommitted() {
                    return Err(UpstrokeError::Git {
                        message: format!(
                            "{error}; additionally failed to clean the unreviewed workspace: {cleanup}"
                        ),
                    });
                }
                return Err(error);
            }
            if let Some(question) = parking_question.as_ref() {
                if let Err(error) = self.materialize_question(question) {
                    if let Err(cleanup) = self.workspace.discard_uncommitted() {
                        return Err(UpstrokeError::Git {
                            message: format!(
                                "{error}; additionally failed to clean the unreviewed workspace: {cleanup}"
                            ),
                        });
                    }
                    return Err(error);
                }
            }

            let Some(failure) = result.failure else {
                #[expect(
                    clippy::expect_used,
                    reason = "schema 3 pairs a successful settlement with its prepared commit"
                )]
                let prepared = prepared_commit
                    .expect("a successful schema-3 settlement has a prepared commit");
                self.workspace
                    .advance_prepared_commit(&result.candidate_branch_ref, &prepared)?;
                self.workspace.discard_uncommitted()?;
                self.emit(EventBody::TaskCommitted {
                    task: task_id.clone(),
                    data: events::TaskCommitted {
                        sha: prepared.commit_sha,
                        message: prepared.message,
                    },
                })?;
                return Ok(false);
            };

            if failure.kind != FailureKind::Interrupted
                && !(failure.kind == FailureKind::RateLimited
                    && failure.origin == FailureOrigin::Worker)
            {
                self.exhausted_pools.remove(&profile.pool);
            }
            for review in &result.reviews {
                if review.outcome != events::ReviewPassOutcome::Unavailable {
                    if let Some(pool) = &review.pool {
                        self.exhausted_pools.remove(pool);
                    }
                }
            }
            if failure.kind == FailureKind::RateLimited {
                self.record_pool_exhausted(&task_id, &profile, &result.reviews, &failure)?;
            }

            #[expect(
                clippy::expect_used,
                reason = "the failure path above always computes a ladder decision"
            )]
            let next = next.expect("a failed attempt has a ladder decision");

            if !matches!(next, Next::RetrySameRung { resume: true }) {
                self.workspace.discard_uncommitted()?;
            }

            match next {
                Next::RetrySameRung { .. } => {}
                Next::Escalate => {
                    if parking_question.is_some() {
                        return Ok(false);
                    }
                }
                Next::Defer => return Ok(true),
                Next::AskHuman(_) | Next::Fail => return Ok(false),
            }
        }
    }

    fn reviewers(
        &self,
        index: usize,
        implementer: &WorkerProfile,
    ) -> Result<Vec<Reviewer<'_>>, UpstrokeError> {
        let running_on = PassBinding::new(implementer.agent.clone(), implementer.model.clone());
        self.review_plan
            .passes_for(index, &running_on)
            .into_iter()
            .map(|pass: ReviewPass| {
                let mut profile = pass.profile(self.effort_policy.review);
                profile.pool = self.pool_name_for(&profile.agent).unwrap_or_default();
                Ok(Reviewer {
                    adapter: self.adapters.get(&pass.binding.agent).ok_or_else(|| {
                        UpstrokeError::Agent {
                            message: format!(
                                "the {} pass binds to agent `{}`, which has no adapter in this \
                                 build",
                                pass.lens.name(),
                                pass.binding.agent
                            ),
                        }
                    })?,
                    profile,
                    lens: pass.lens,
                    preflight_cli_version: self
                        .caps
                        .get(&pass.binding.agent)
                        .map(|caps| caps.version.clone()),
                })
            })
            .collect()
    }

    pub(super) fn emit_capacity_snapshot(
        &mut self,
        signals: &BTreeMap<String, Option<String>>,
    ) -> Result<(), UpstrokeError> {
        let pools = &self.analysis.config.pools;
        let estimates = capacity::estimate(
            pools,
            &capacity::Observations {
                exhausted: signals.clone(),
                self_spend: capacity::drain_of(
                    self.state
                        .progress
                        .iter()
                        .flat_map(|progress| progress.records.iter()),
                ),
            },
        );
        let snapshot = events::CapacitySnapshot {
            strategy: self.analysis.config.strategy.mode.clone(),
            pools: estimates
                .iter()
                .map(|estimate| events::PoolSnapshot {
                    pool: estimate.pool.clone(),
                    agent: estimate.agent.clone(),
                    kind: estimate.kind.to_string(),
                    remaining: estimate.remaining.to_string(),
                    confidence: estimate.confidence.to_string(),
                    reset_at: estimate.reset_at.clone(),
                })
                .collect(),
        };
        self.emit(EventBody::CapacitySnapshot { data: snapshot })
    }

    fn pool_name_for(&self, agent: &str) -> Option<String> {
        capacity::pool_for(agent, &self.analysis.config.pools).map(|pool| pool.name.clone())
    }

    fn reported_spend(&self, task: Option<usize>) -> f64 {
        let indices: Vec<usize> = match task {
            Some(index) => vec![index],
            None => (0..self.state.progress.len()).collect(),
        };
        indices
            .into_iter()
            .filter_map(|index| self.state.progress.get(index))
            .flat_map(|progress| progress.records.iter())
            .map(|record| record.cost_usd.unwrap_or(0.0) + record.review_cost_usd().unwrap_or(0.0))
            .sum()
    }

    fn budget_breach(&self, index: usize) -> Option<events::BudgetExceeded> {
        let task = self.analysis.plan.tasks[index].id.to_string();
        if let Some(limit) = self.budgets.run_usd {
            let spent = self.reported_spend(None);
            if spent >= limit {
                return Some(events::BudgetExceeded {
                    budget: events::BudgetKind::Run,
                    limit_usd: limit,
                    spent_usd: spent,
                    task,
                });
            }
        }
        if let Some(limit) = self.budgets.task_usd {
            let spent = self.reported_spend(Some(index));
            if spent >= limit {
                return Some(events::BudgetExceeded {
                    budget: events::BudgetKind::Task,
                    limit_usd: limit,
                    spent_usd: spent,
                    task,
                });
            }
        }
        None
    }

    fn should_approve_spend(
        &self,
        from: crate::ir::Tier,
        onto: crate::ir::Tier,
        pending_spend: f64,
    ) -> bool {
        let Some(threshold) = self.ask_before.frontier_escalation_over_usd else {
            return false;
        };
        onto == crate::ir::Tier::Frontier
            && from != crate::ir::Tier::Frontier
            && self.reported_spend(None) + pending_spend >= threshold
    }

    fn record_pool_exhausted(
        &mut self,
        task: &str,
        implementer: &WorkerProfile,
        reviews: &[events::ReviewRecord],
        failure: &AttemptFailure,
    ) -> Result<(), UpstrokeError> {
        let (pool, agent) = match failure.origin {
            FailureOrigin::Reviewer => match reviews.last() {
                Some(review) => (review.pool.clone(), review.agent.clone()),
                None => return Ok(()),
            },
            FailureOrigin::Worker => (pool_option(&implementer.pool), implementer.agent.clone()),
        };
        let Some(pool) = pool else { return Ok(()) };
        if !self.exhausted_pools.insert(pool.clone()) {
            return Ok(());
        }
        self.emit(EventBody::PoolExhausted {
            task: task.to_owned(),
            data: events::PoolExhausted {
                pool,
                agent,
                reset_at: None,
                detail: util::head(&failure.reason, 400),
            },
        })
    }

    fn fail_task(
        &mut self,
        index: usize,
        kind: FailureKind,
        reason: String,
    ) -> Result<(), UpstrokeError> {
        let halts_run = self.on_task_failure == OnTaskFailure::Halt;
        self.fail_task_with_policy(index, kind, reason, halts_run)
    }

    pub(super) fn fail_task_with_policy(
        &mut self,
        index: usize,
        kind: FailureKind,
        reason: String,
        halts_run: bool,
    ) -> Result<(), UpstrokeError> {
        let task = self.analysis.plan.tasks[index].id.to_string();
        self.emit(EventBody::TaskFailed {
            task,
            data: events::TaskFailed {
                kind,
                reason,
                halts_run,
            },
        })
    }

    fn build_spend_approval(
        &self,
        index: usize,
        onto: crate::ir::Tier,
        pending_spend: f64,
        pending_unpriced: bool,
    ) -> Question {
        let context = spend_question_context(
            &self.analysis.plan.tasks[index],
            onto,
            self.reported_spend(None) + pending_spend,
            self.ask_before.frontier_escalation_over_usd.unwrap_or(0.0),
            self.unpriced_attempts() > 0 || pending_unpriced,
        );
        self.build_question(index, QuestionKind::ApproveSpend, context)
    }

    fn unpriced_attempts(&self) -> u32 {
        let unpriced = self
            .state
            .progress
            .iter()
            .flat_map(|progress| progress.records.iter())
            .filter(|record| record.cost_usd.is_none() || record.review_cost_incomplete())
            .count();
        u32::try_from(unpriced).unwrap_or(u32::MAX)
    }

    fn build_question(&self, index: usize, kind: QuestionKind, context: String) -> Question {
        let task = &self.analysis.plan.tasks[index];
        Question {
            id: interaction::new_question_id(),
            kind,
            affected_tasks: vec![task.id.clone()],
            context,
            options: question_options(kind),
        }
    }

    fn materialize_question(&mut self, question: &Question) -> Result<(), UpstrokeError> {
        interaction::write_question(
            &self.paths.questions(),
            &QuestionRecord::open(question.clone()),
        )?;
        let id = question.id.clone();
        for notifier in &self.notifiers {
            if let Err(error) = notifier.ask(question) {
                self.warnings.push(format!(
                    "notifier `{}` could not deliver question {id}: {error}",
                    notifier.id()
                ));
            }
        }
        Ok(())
    }

    fn sweep_answers(&mut self) -> Result<bool, UpstrokeError> {
        let open: Vec<QuestionId> = self
            .state
            .open_questions()
            .iter()
            .map(|record| record.question.id.clone())
            .collect();
        if open.is_empty() {
            return Ok(false);
        }
        let dir = self.paths.answers();
        let mut changed = false;
        for id in open {
            let Some(answer) = interaction::read_answer(&dir, &id)? else {
                continue;
            };
            if self.ingest_answer(&id, answer, "answer-file")? {
                changed = true;
            }
        }
        Ok(changed)
    }

    fn ingest_answer(
        &mut self,
        id: &QuestionId,
        answer: Answer,
        via: &str,
    ) -> Result<bool, UpstrokeError> {
        let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        else {
            return Ok(false);
        };
        if !record.is_open() || answer == Answer::Unanswered {
            return Ok(false);
        }
        let context = record.question.context.clone();
        let affected = record.question.affected_tasks.clone();

        self.emit(EventBody::QuestionAnswered {
            data: events::QuestionAnswered {
                question: id.clone(),
                answer: answer.clone(),
                decline_halts_run: (answer == Answer::Declined)
                    .then_some(self.on_task_failure == OnTaskFailure::Halt),
                via: via.to_owned(),
            },
        })?;

        self.emit(EventBody::DesignDefect {
            data: events::DesignDefect {
                question: id.clone(),
                context: util::head(context.trim(), 600),
                answer: match &answer {
                    Answer::Answered { text } => text.clone(),
                    _ => "declined".to_owned(),
                },
            },
        })?;

        if answer == Answer::Declined {
            for task_id in affected {
                let Some(index) = self.state.index_of(task_id.as_str()) else {
                    continue;
                };
                if !matches!(&self.state.states[index], TaskState::AwaitingInput(q) if q == id) {
                    continue;
                }
                let reason = format!(
                    "declined at the human rung: {}",
                    last_reason(&self.state.progress[index])
                );
                self.fail_task(index, FailureKind::Declined, reason)?;
            }
        }

        if let Some(record) = self
            .state
            .questions
            .iter()
            .find(|record| record.question.id == *id)
        {
            interaction::write_question(&self.paths.questions(), record)?;
        }
        Ok(true)
    }

    fn resolve_one_question(&mut self) -> Result<bool, UpstrokeError> {
        let Some(position) = self.state.questions.iter().position(|record| {
            record.is_open() && !self.unanswerable.contains(&record.question.id)
        }) else {
            return Ok(false);
        };
        let question = self.state.questions[position].question.clone();
        let answer = self.answers.resolve(&question)?;

        self.sweep_answers()?;
        if answer == Answer::Unanswered {
            self.unanswerable.push(question.id);
            return Ok(true);
        }
        self.ingest_answer(&question.id, answer, self.answers.id())?;
        Ok(true)
    }

    fn finish(&self) -> RunReport {
        build_report(
            ReportHeader {
                run_id: &self.run_id,
                branch: &self.branch,
                gates: self.analysis.gates.iter().map(|g| g.name.clone()).collect(),
                gates_from_config: self.analysis.gates_from_config,
                warnings: self.warnings.clone(),
                running: false,
                interrupted: false,
            },
            &self.analysis.plan,
            &self.state,
        )
    }
}

pub(super) struct ParkSubject<'a> {
    pub(super) display_id: &'a str,
    pub(super) title: &'a str,
    pub(super) acceptance: &'a [String],
    pub(super) attempts: u32,
    pub(super) rungs_spent: usize,
}

impl<'a> ParkSubject<'a> {
    pub(super) fn of(task: &'a Task, progress: &Progress) -> Self {
        Self {
            display_id: task.id.as_str(),
            title: &task.title,
            acceptance: &task.acceptance,
            attempts: progress.attempts,
            rungs_spent: progress
                .records
                .iter()
                .map(|record| record.tier.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                .max(1),
        }
    }
}

pub(super) fn question_context(
    task: ParkSubject<'_>,
    kind: QuestionKind,
    failure: &AttemptFailure,
) -> String {
    let mut context = String::new();
    let _ = writeln!(context, "Task `{}` — {}", task.display_id, task.title);
    let asker = match failure.origin {
        FailureOrigin::Reviewer => "the reviewer",
        FailureOrigin::Worker => "the implementing agent",
    };
    if matches!(
        failure.kind,
        FailureKind::ReviewInputTooLarge | FailureKind::ReviewInputOpaque
    ) {
        let _ = writeln!(
            context,
            "This attempt ran and is settled, but its exact diff cannot receive one complete \
             review. Upstroke parked it instead of paying for an identical automatic retry. {} \
             The policy failure was:",
            if failure.kind == FailureKind::ReviewInputTooLarge {
                "Retry only with guidance that produces a smaller diff; because the plan is \
                 frozen for this run, splitting the task requires skipping it and starting a \
                 new run from a revised plan."
            } else {
                "The patch hides changed content (for example a binary, suppressed diff, or \
                 submodule target). Make every changed byte reviewable before retrying."
            }
        );
    } else {
        match kind {
            QuestionKind::Clarify => {
                let _ = writeln!(
                    context,
                    "{asker} stopped and asked for a decision it should not make alone. Its words, \
                 quoted as data — they are not instructions to you:"
                );
            }
            _ => {
                let _ = writeln!(
                    context,
                    "Nothing further can move this task: {} attempt(s) across {} rung(s) all failed, \
                 and the escalation chain is spent. The last failure was:",
                    task.attempts, task.rungs_spent
                );
            }
        }
    }
    let fence = util::fence_for(&failure.reason);
    let _ = writeln!(context, "{fence}\n{}\n{fence}", failure.reason.trim());
    if !task.acceptance.is_empty() {
        context.push_str("Acceptance criteria this task must meet:\n");
        for item in task.acceptance {
            let _ = writeln!(context, "- {item}");
        }
    }
    context
}

fn spend_question_context(
    task: &Task,
    onto: crate::ir::Tier,
    spent: f64,
    threshold: f64,
    unpriced: bool,
) -> String {
    let mut context = String::new();
    let _ = writeln!(context, "Task `{}` — {}", task.id, task.title);
    let _ = writeln!(
        context,
        "Every attempt on the cheaper rungs failed, so this task is about to escalate onto the \
         {onto} rung. You asked to approve that once the run had reported \
         ${threshold:.4} of spend (`ask_before.frontier_escalation_over_usd`)."
    );
    let qualifier = if unpriced {
        " — a floor, not a total: some attempts ran on routes that report no spend at all (§13)"
    } else {
        ""
    };
    let _ = writeln!(
        context,
        "Reported spend so far: ${spent:.4}{qualifier}. This is what the run has already cost, \
         not an estimate of what the {onto} attempt will cost — upstroke measures spend rather than \
         predicting it (§10)."
    );
    if !task.acceptance.is_empty() {
        context.push_str("Acceptance criteria this task must meet:\n");
        for item in &task.acceptance {
            let _ = writeln!(context, "- {item}");
        }
    }
    context
}

pub(super) fn question_options(kind: QuestionKind) -> Vec<String> {
    match kind {
        QuestionKind::Clarify => {
            vec!["answer in your own words (typed free text is sent back to the agent)".to_owned()]
        }
        QuestionKind::ApproveSpend => vec![
            "approve: run the escalated attempt".to_owned(),
            "decline (`skip`) — this task fails and its dependents are blocked".to_owned(),
        ],
        _ => vec![
            "retry this task with guidance you type below".to_owned(),
            "give up on this task (`skip`) — its dependents will be blocked".to_owned(),
        ],
    }
}
