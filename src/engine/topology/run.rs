//! Extended notes: `docs/internals/engine/topology/run.md`

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::ir::{Answer, Question, QuestionId};
use crate::review;
use crate::topology::events::TopologyEventBody;

use crate::events::AttemptRecord;
use crate::interaction::Sleeper;
use crate::topology::events::{
    AttemptNumber, CandidateLeaseEffect, CommitSha, FrozenQuestion, GenerationId, SessionId,
    TopologyEvent,
};
use crate::topology::fold::{FrozenInputs, TopologyFold};
use crate::topology::registry::TaskKey;
use crate::workspace_manager::WorkspaceManager;

use super::attempt::{
    Assessment, AttemptContext, AttemptPlan, AttemptPlans, AttemptSite, Capture, InputsRequest,
    Judgement, Judging, PlanRequest, ReviewInputPolicy, ReviewPasses,
};
use super::candidate::{
    CandidateJournal, JudgedTree, append_candidate_created, append_candidate_prepared,
    create_candidates_ref, pin_candidate, reclaim_after_creation, write_candidate_commit,
};
use super::dispatch::{
    DispatchKind, DispatchRequest, Dispatched, EventEmitter, OpenGeneration, dispatch,
    resume_open_no_attempt, task_slot,
};
use super::emit::{EmitFailure, EmitState, RunIdentity, emit};
use super::identity::{InvocationLedger, ReservationKind, Reservations, SlotAssertion};
use super::recover::RunHandle;
use super::seams::{IdSource, TimeSource, TopologyHooks};
use super::select::{Admitted, Ceiling, Spend, Step, checkpoint, select};
use super::settle::{
    Deferral, FinishedAttempt, ManagedWorktrees, RetryOutcome, RetryRequest, retry, settle_failed,
};

pub struct RunEmitter<'a> {
    pub identity: &'a RunIdentity,
    pub state: EmitState<'a>,
    pub clock: &'a dyn TimeSource,
}

impl EventEmitter for RunEmitter<'_> {
    fn emit(
        &mut self,
        body: TopologyEventBody,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), EmitFailure> {
        emit(self.identity, &mut self.state, self.clock, body, hooks)?;
        Ok(())
    }
}

struct RunJournal<'a, 'h> {
    emitter: RunEmitter<'a>,
    hooks: &'h mut dyn TopologyHooks,
    invocations: &'h mut InvocationLedger,
}

impl CandidateJournal for RunJournal<'_, '_> {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
        self.emitter
            .emit(body, self.hooks)
            .map_err(|failure| failure.discharging(self.invocations))
    }

    fn fold(&self) -> &TopologyFold {
        self.emitter.state.fold
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoopBranch {
    IngestAnswers,
    Integration,
    ReadyRetry,
    ReadyDispatch,
    DeferBackoff,
    HardBlock,
    Closure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Performed,
    RefusedByCheckpoint,
    NotYetImplemented,
    NotThisSlice {
        slice: &'static str,
        citation: &'static str,
    },
    #[allow(dead_code)]
    PartlyImplemented {
        performs: &'static str,
        owes: &'static str,
    },
}

impl LoopBranch {
    pub const ALL: [Self; 7] = [
        Self::IngestAnswers,
        Self::Integration,
        Self::ReadyRetry,
        Self::ReadyDispatch,
        Self::DeferBackoff,
        Self::HardBlock,
        Self::Closure,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::IngestAnswers => "ingest answers",
            Self::Integration => "integration",
            Self::ReadyRetry => "ready_retry",
            Self::ReadyDispatch => "ready dispatch",
            Self::DeferBackoff => "defer backoff",
            Self::HardBlock => "hard block",
            Self::Closure => "run-end closure",
        }
    }

    #[must_use]
    pub const fn disposition(self) -> Disposition {
        match self {
            Self::Integration | Self::Closure => Disposition::RefusedByCheckpoint,
            Self::DeferBackoff => Disposition::Performed,
            Self::ReadyDispatch => Disposition::Performed,
            Self::ReadyRetry => Disposition::Performed,
            Self::HardBlock => Disposition::Performed,
            Self::IngestAnswers => Disposition::NotThisSlice {
                slice: "PR9",
                citation: "`pr_sequence[8]` does not contain the word `answer`; PR8 still \
                           refuses `repair-admission answers before any append`; PR9 owns \
                           `question_answered`, `T-ANSWER`, and `AwaitingInput -> Pending via \
                           validated answer`. PR7's `replay_recovery` never names `T-ANSWER`",
            },
        }
    }

    #[must_use]
    pub const fn of(step: &Step) -> Option<Self> {
        match step {
            Step::Poisoned => None,
            Step::BudgetExceeded(_) => None,
            Step::Integrate { .. } => Some(Self::Integration),
            Step::Retry { .. } => Some(Self::ReadyRetry),
            Step::Dispatch { .. } => Some(Self::ReadyDispatch),
            Step::Backoff => Some(Self::DeferBackoff),
            Step::HardBlock { .. } => Some(Self::HardBlock),
            Step::Closure(_) => Some(Self::Closure),
        }
    }

    #[allow(dead_code)]
    pub fn owes(self, clause: &str) -> UpstrokeError {
        UpstrokeError::Refused {
            message: format!(
                "the schema-4 run loop's `{}` branch reached a case this build does not \
                 implement: {clause}. Nothing was appended for it",
                self.label()
            ),
        }
    }

    pub fn unimplemented(self) -> UpstrokeError {
        match self.disposition() {
            Disposition::PartlyImplemented { performs, owes } => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop's `{}` branch performed {performs}, and this build \
                     does not {owes}",
                    self.label()
                ),
            },
            Disposition::NotThisSlice { slice, citation } => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop's `{}` branch belongs to {slice}, not to this build: \
                     {citation}. No effect was performed and no event was appended",
                    self.label()
                ),
            },
            _ => UpstrokeError::Refused {
                message: format!(
                    "the schema-4 run loop selected its `{}` branch, which this build does not \
                     implement yet; no effect was performed and no event was appended",
                    self.label()
                ),
            },
        }
    }
}

pub struct RunSeams<'a> {
    pub manager: &'a WorkspaceManager,
    pub clock: &'a dyn TimeSource,
    pub sleeper: &'a dyn Sleeper,
    pub runner: &'a dyn crate::runner::Runner,
    pub adapters: &'a dyn crate::agent::AdapterSource,
    pub paths: &'a crate::rundir::RunPaths,
    pub plans: &'a dyn AttemptPlans,
    pub reviews: &'a dyn ReviewPasses,
    pub input_policy: &'a dyn ReviewInputPolicy,
    pub answers: &'a dyn crate::interaction::AnswerSource,
    pub ids: &'a dyn IdSource,
    pub halts_run: bool,
}

#[derive(Debug, Clone)]
struct RunAs {
    feedback: Vec<crate::events::Feedback>,
    attempt: AttemptNumber,
    rung: u32,
    resume_session: Option<SessionId>,
    announced: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brief {
    per_task: BTreeMap<TaskKey, Vec<crate::events::Feedback>>,
}

impl Brief {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn lines(&self, key: TaskKey) -> Vec<crate::events::Feedback> {
        self.per_task.get(&key).cloned().unwrap_or_default()
    }

    pub fn record(&mut self, key: TaskKey, record: &AttemptRecord) {
        let Some(failure) = record.failure.as_ref() else {
            return;
        };
        self.per_task
            .entry(key)
            .or_default()
            .push(crate::events::Feedback {
                attempt: record.attempt,
                tier: record.tier.clone(),
                summary: failure.reason.clone(),
                detail: failure.detail.clone(),
                human: false,
            });
    }

    #[must_use]
    pub fn replay(events: &[TopologyEvent]) -> Self {
        let mut brief = Self::new();
        for event in events {
            if let TopologyEventBody::AttemptFinished { data } = &event.body {
                brief.record(data.key, &data.record);
            }
        }
        brief
    }
}

#[derive(Debug, Clone)]
struct Retained {
    tree: String,
}

#[derive(Debug, Clone, Copy)]
struct Produced<'a> {
    capture: &'a Capture,
    assessed: &'a Assessment,
    judgement: &'a Judgement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Settled {
        key: TaskKey,
        accepted: bool,
        spent_attempt: bool,
    },
    GenerationClosed {
        key: TaskKey,
    },
    Blocked {
        questions: usize,
    },
    Waited {
        waited_ms: u64,
        round: u32,
    },
    BudgetExceeded,
}

pub struct TopologyRun {
    handle: RunHandle,
    identity: RunIdentity,
    reservations: Reservations,
    invocations: InvocationLedger,
    warnings: Vec<String>,
    ceiling: Ceiling,
    spend: Spend,
    deferral: Deferral,
    slots: SlotAssertion,
    retained: BTreeMap<TaskKey, Retained>,
    brief: Brief,
}

impl TopologyRun {
    #[must_use]
    pub fn resumed(handle: RunHandle, inputs: FrozenInputs, ceiling: Ceiling) -> Self {
        let identity = RunIdentity {
            run_id: handle.started.run_id.clone(),
            inputs,
            committed_first_line_sha256: Some(handle.committed_first_line_sha256.clone()),
        };
        let spend = Spend::replay(&handle.events);
        let brief = Brief::replay(&handle.events);
        Self {
            handle,
            identity,
            reservations: Reservations::new(),
            invocations: InvocationLedger::new(),
            warnings: Vec::new(),
            ceiling,
            spend,
            deferral: Deferral::default_backoff(),
            slots: SlotAssertion::new(),
            retained: BTreeMap::new(),
            brief,
        }
    }

    pub fn commitment_digest(&self) -> Option<&str> {
        self.identity.committed_first_line_sha256.as_deref()
    }

    #[must_use]
    pub fn holds_entitlement(&mut self) -> bool {
        self.reservations.cancel_any()
    }

    pub fn invocations_balance(&self) -> bool {
        self.invocations.balances()
    }

    pub const fn spend(&self) -> &Spend {
        &self.spend
    }

    #[must_use]
    pub fn fold(&self) -> &TopologyFold {
        &self.handle.fold
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    #[must_use]
    pub fn entitlements_held(&self) -> u32 {
        self.reservations.entitlements_held()
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn defer_round(&self) -> u32 {
        self.deferral.round()
    }

    pub fn step(
        &mut self,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Progress, UpstrokeError> {
        let selected = select(&self.handle.fold, &self.ceiling, &self.spend);
        let admitted = checkpoint(selected)?;
        match admitted {
            Admitted::BudgetExceeded(exceeded) => {
                self.emit(
                    TopologyEventBody::BudgetExceeded { data: *exceeded },
                    seams,
                    hooks,
                )?;
                Ok(Progress::BudgetExceeded)
            }
            Admitted::Backoff => {
                let elapsed = self.deferral.wait(seams.sleeper);
                let (waited_ms, round) = (elapsed.waited_ms, elapsed.round);
                self.emit(
                    TopologyEventBody::DeferWaitElapsed { data: elapsed },
                    seams,
                    hooks,
                )?;
                Ok(Progress::Waited { waited_ms, round })
            }
            Admitted::Retry {
                key, generation, ..
            } => self.retry_ready(key, generation, seams, hooks),
            Admitted::Dispatch {
                key,
                generation,
                continuing,
            } => {
                let dispatched = if continuing {
                    self.continue_open(key, generation, seams, hooks)?
                } else {
                    self.dispatch_ready(key, generation, seams, hooks)?
                };
                let (plan, capture, assessed, judgement) = self.attempt(
                    dispatched.site(),
                    RunAs {
                        attempt: Self::FIRST_ATTEMPT,
                        rung: self.ladder_position(key)?.0,
                        resume_session: None,
                        feedback: self.brief.lines(key),
                        announced: false,
                    },
                    seams,
                    hooks,
                )?;
                let accepted = judgement.accepted();
                let spent_attempt = self.settle(
                    dispatched.site(),
                    &plan,
                    Produced {
                        capture: &capture,
                        assessed: &assessed,
                        judgement: &judgement,
                    },
                    seams,
                    hooks,
                )?;
                Ok(Progress::Settled {
                    key,
                    accepted,
                    spent_attempt,
                })
            }
            Admitted::HardBlock { questions } => self.hard_block(&questions, seams),
        }
    }

    fn dispatch_ready(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Dispatched, UpstrokeError> {
        let request = self.dispatch_request(key, generation)?;

        self.reservations.take(key, ReservationKind::Dispatch)?;

        let dispatched = {
            let mut emitter = RunEmitter {
                identity: &self.identity,
                state: EmitState {
                    fold: &mut self.handle.fold,
                    log: &mut self.handle.log,
                    reservations: &mut self.reservations,
                    warnings: &mut self.warnings,
                },
                clock: seams.clock,
            };
            dispatch(seams.manager, hooks, &mut emitter, &request)
        };

        match dispatched {
            Ok(dispatched) => {
                self.reservations.convert(key, ReservationKind::Dispatch)?;
                self.deferral.progressed();
                Ok(dispatched)
            }
            Err(error) => {
                let _ = self.reservations.cancel(key, ReservationKind::Dispatch);
                Err(error.discharging(&mut self.invocations))
            }
        }
    }

    fn hard_block(
        &mut self,
        questions: &[QuestionId],
        seams: &RunSeams<'_>,
    ) -> Result<Progress, UpstrokeError> {
        for id in questions {
            let question = self.open_question(id)?;
            match seams.answers.resolve(&question)? {
                Answer::Unanswered => {}
                Answer::Answered { .. } | Answer::Declined => {
                    return Err(UpstrokeError::Refused {
                        message: format!(
                            "question {} was answered, and ingesting an answer is PR9's: \
                             `question_answered` and `T-ANSWER` are that slice's and this one's \
                             contract does not name them. Refused before any append",
                            id.0
                        ),
                    });
                }
            }
        }
        Ok(Progress::Blocked {
            questions: questions.len(),
        })
    }

    fn open_question(&self, id: &QuestionId) -> Result<Question, UpstrokeError> {
        let open = self
            .handle
            .fold
            .open_questions()
            .and_then(|open| open.get(id))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("question {} is not open in this run's fold", id.0),
            })?;
        let frozen = &open.question;
        Ok(Question {
            id: frozen.id.clone(),
            kind: frozen.kind,
            affected_tasks: vec![crate::ir::TaskId(self.display_id(frozen.key)?)],
            context: frozen.context.clone(),
            options: frozen.options.clone(),
        })
    }

    fn continue_open(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Dispatched, UpstrokeError> {
        let base = self
            .handle
            .fold
            .task(key)
            .and_then(|task| task.generations.iter().find(|held| held.id == generation))
            .map(|held| held.base_sha.clone())
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no generation {} to continue",
                    key.index(),
                    generation.0
                ),
            })?;
        let slot = task_slot(key, generation);
        let open = OpenGeneration {
            key,
            generation,
            base: base.clone(),
            slot: slot.clone(),
            source: None,
        };
        resume_open_no_attempt(seams.manager, hooks, &open)?;
        self.deferral.progressed();
        Ok(Dispatched {
            key,
            generation,
            base,
            worktree: seams.manager.slot_path(&slot),
            slot,
            kind: DispatchKind::Ordinary {
                paths: self.handle.fold.predicted_region(key).ok_or_else(|| {
                    UpstrokeError::Refused {
                        message: format!("task {} has no predicted region", key.index()),
                    }
                })?,
            },
        })
    }

    fn retry_ready(
        &mut self,
        key: TaskKey,
        generation: GenerationId,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<Progress, UpstrokeError> {
        let position = self.ladder_position(key)?;
        let held = self
            .retained
            .get(&key)
            .cloned()
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has a retained generation this process did not retain, so the tree \
                     its retry must re-gate is not known here. A fresh process closes a retained \
                     generation in recovery rather than continuing it",
                    key.index()
                ),
            })?;
        let binding = self
            .handle
            .fold
            .frozen_rung_binding(key, position.0)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no rung {} in its frozen ladder",
                    key.index(),
                    position.0
                ),
            })?;
        let slot_for_run = task_slot(key, generation);
        let slot = slot_for_run.clone();
        let pool = seams.plans.pool_for(&binding.agent);

        let outcome = {
            let worktrees = ManagedWorktrees::new(seams.manager);
            retry(
                &self.handle.fold,
                &mut self.reservations,
                &worktrees,
                hooks.effects(),
                &RetryRequest {
                    key,
                    slot,
                    retained_tree: held.tree.clone(),
                    binding,
                    rung: position.0,
                    pool: pool.clone(),
                    materialization: None,
                },
            )?
        };

        match outcome {
            RetryOutcome::Start(started) => {
                let run_as = RunAs {
                    attempt: started.attempt,
                    rung: started.rung,
                    resume_session: started.resume_session.clone(),
                    feedback: self.brief.lines(key),
                    announced: true,
                };
                self.emit(
                    TopologyEventBody::AttemptStarted { data: *started },
                    seams,
                    hooks,
                )?;
                self.reservations.convert(key, ReservationKind::Retry)?;
                self.deferral.progressed();

                let base = self
                    .handle
                    .fold
                    .task(key)
                    .and_then(|task| task.generations.iter().find(|held| held.id == generation))
                    .map(|held| held.base_sha.clone())
                    .ok_or_else(|| UpstrokeError::Refused {
                        message: format!(
                            "generation {} of task {} left the fold mid-retry",
                            generation.0,
                            key.index()
                        ),
                    })?;
                let worktree = seams.manager.slot_path(&slot_for_run);
                let site = AttemptSite {
                    key,
                    generation,
                    base: &base,
                    slot: &slot_for_run,
                    worktree: &worktree,
                };
                let (plan, capture, assessed, judgement) =
                    self.attempt(site, run_as, seams, hooks)?;
                let accepted = judgement.accepted();
                let spent_attempt = self.settle(
                    site,
                    &plan,
                    Produced {
                        capture: &capture,
                        assessed: &assessed,
                        judgement: &judgement,
                    },
                    seams,
                    hooks,
                )?;
                return Ok(Progress::Settled {
                    key,
                    accepted,
                    spent_attempt,
                });
            }
            RetryOutcome::Close { closed, .. } => {
                self.emit(
                    TopologyEventBody::GenerationClosed { data: closed },
                    seams,
                    hooks,
                )?;
                self.retained.remove(&key);
            }
        }
        Ok(Progress::GenerationClosed { key })
    }

    const FIRST_ATTEMPT: crate::topology::events::AttemptNumber =
        crate::topology::events::AttemptNumber(1);

    fn attempt(
        &mut self,
        site: AttemptSite<'_>,
        run_as: RunAs,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(AttemptPlan, Capture, Assessment, Judgement), UpstrokeError> {
        let key = site.key;
        let binding = self
            .handle
            .fold
            .frozen_rung_binding(key, run_as.rung)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "task {} has no rung {} in its frozen ladder, so there is no binding to \
                     run it under",
                    key.index(),
                    run_as.rung
                ),
            })?;

        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?
            .clone();

        let plan = seams.plans.plan(&PlanRequest {
            key,
            entry: &entry,
            attempt: run_as.attempt,
            rung: run_as.rung,
            binding,
            workspace: site.worktree,
            resume_session: run_as.resume_session.clone(),
            feedback: run_as.feedback.clone(),
            materialization_observed: None,
        })?;

        let mut emitter = RunEmitter {
            identity: &self.identity,
            state: EmitState {
                fold: &mut self.handle.fold,
                log: &mut self.handle.log,
                reservations: &mut self.reservations,
                warnings: &mut self.warnings,
            },
            clock: seams.clock,
        };
        let mut cx = AttemptContext {
            manager: seams.manager,
            hooks,
            emitter: &mut emitter,
            runner: seams.runner,
            slots: &mut self.slots,
            ledger: &mut self.invocations,
            adapters: seams.adapters,
            paths: seams.paths,
            reviews: seams.reviews,
            input_policy: seams.input_policy,
        };

        let run = if run_as.announced {
            cx.run_worker(site, &plan)?
        } else {
            cx.start(site, &plan)?
        };
        let capture = cx.capture(site)?;
        let diff = seams
            .manager
            .candidate_diff(site.slot, &capture.parent, &capture.tree)?;

        let assessed = cx.assess(site, &plan, &run, &capture, &diff, entry.spec.kind)?;

        let inputs = seams.plans.inputs(&InputsRequest {
            entry: &entry,
            diff,
        })?;

        let judgement = cx.judge(
            site,
            &plan,
            Judging {
                run: &run,
                capture: &capture,
                assessed: &assessed,
            },
            &inputs,
            &|pass| review::ReviewInvocations {
                pass: run.identities.review_pass(pass, 0),
                reask: run.identities.review_reask(pass, 0),
            },
        )?;
        Ok((plan, capture, assessed, judgement))
    }

    fn settle(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        produced: Produced<'_>,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<bool, UpstrokeError> {
        let Produced {
            capture,
            assessed,
            judgement,
        } = produced;
        let record = crate::engine::classify::attempt_record(
            plan.attempt.0,
            crate::engine::classify::AttemptFacts {
                tier: plan.binding.tier,
                model: &plan.binding.model,
                pool: plan.pool.clone(),
                resumed: plan.resume_session.is_some(),
                outcome: &assessed.outcome,
                reviews: &judgement.reviews,
                failure: judgement.failure.as_ref(),
                feedback: crate::engine::classify::FeedbackCarrier::AttemptRecord,
            },
        );

        let Some(failure) = judgement.failure.as_ref() else {
            self.promote_candidate(site, plan, capture, record, seams, hooks)?;
            return Ok(crate::ladder::spends_allowance(None));
        };

        let policy = self.ladder_policy(site.key)?;

        let defers = self.deferrals_recorded(site.key)?;
        let position = self.ladder_position(site.key)?;

        let attempts_on_rung =
            position
                .1
                .saturating_add(u32::from(crate::ladder::spends_allowance(Some(
                    crate::ladder::FailureShape::of(failure),
                ))));

        let next = crate::ladder::next_step(
            failure,
            &crate::ladder::LadderState {
                rung: position.0 as usize,
                attempts_on_rung,
                defers,
                resumable: plan.session_resume && assessed.outcome.session_id.is_some(),
            },
            &policy,
        );
        let question = match next {
            crate::ladder::Next::AskHuman(kind) => {
                Some(self.park_question(site.key, attempts_on_rung, kind, failure, seams.ids)?)
            }
            _ => None,
        };

        let settled = settle_failed(
            &self.handle.fold,
            &FinishedAttempt {
                key: site.key,
                generation: site.generation,
                attempt: plan.attempt,
                record,
                next,
                session: assessed.outcome.session_id.clone().map(SessionId),
                question,
                halts_run: seams.halts_run,
                defers,
                reason: failure.reason.clone(),
                rung: position.0.saturating_add(1),
            },
        )?;

        if matches!(
            settled.event.settlement,
            crate::topology::events::AttemptSettlement::Retained { .. }
        ) {
            self.retained.insert(
                site.key,
                Retained {
                    tree: capture.tree.clone(),
                },
            );
        }
        self.spend.record(site.key, &settled.event.record);
        self.brief.record(site.key, &settled.event.record);
        self.emit(
            TopologyEventBody::AttemptFinished {
                data: Box::new(settled.event),
            },
            seams,
            hooks,
        )?;
        Ok(settled.spent_attempt)
    }

    fn promote_candidate(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        capture: &Capture,
        record: AttemptRecord,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), UpstrokeError> {
        let key = site.key;
        let actual_paths = seams.manager.changed_paths(site.slot, &capture.parent)?;

        let judged = JudgedTree {
            key,
            generation: site.generation,
            attempt: Box::new(record.clone()),
            base_sha: site.base.clone(),
            tree_sha: CommitSha(capture.tree.clone()),
            message: format!(
                "upstroke: {} attempt {}",
                self.display_id(key)?,
                plan.attempt.0
            ),
            actual_paths,
            lease_effect: CandidateLeaseEffect::ReplacesPredicted {
                paths: seams.manager.changed_paths(site.slot, &capture.parent)?,
            },
        };

        let run_id = self.identity.run_id.clone();
        let unpinned = write_candidate_commit(seams.manager, hooks, &run_id, judged)?;
        let pinned = pin_candidate(seams.manager, hooks, unpinned)?;

        self.spend.record(key, &record);
        self.brief.record(key, &record);

        let promoting = self.with_journal(seams, hooks, |journal| {
            append_candidate_prepared(journal, pinned)
        })?;
        let referenced = create_candidates_ref(seams.manager, hooks, promoting)?;
        let created = self.with_journal(seams, hooks, |journal| {
            append_candidate_created(journal, referenced)
        })?;
        reclaim_after_creation(seams.manager, hooks, site.slot, created)?;
        Ok(())
    }

    fn with_journal<T>(
        &mut self,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
        run: impl FnOnce(&mut RunJournal<'_, '_>) -> Result<T, UpstrokeError>,
    ) -> Result<T, UpstrokeError> {
        let mut journal = RunJournal {
            emitter: RunEmitter {
                identity: &self.identity,
                state: EmitState {
                    fold: &mut self.handle.fold,
                    log: &mut self.handle.log,
                    reservations: &mut self.reservations,
                    warnings: &mut self.warnings,
                },
                clock: seams.clock,
            },
            hooks,
            invocations: &mut self.invocations,
        };
        run(&mut journal)
    }

    fn display_id(&self, key: TaskKey) -> Result<String, UpstrokeError> {
        Ok(self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?
            .display_id
            .as_str()
            .to_owned())
    }

    fn park_question(
        &self,
        key: TaskKey,
        attempts_on_rung: u32,
        kind: crate::ir::QuestionKind,
        failure: &crate::ladder::AttemptFailure,
        ids: &dyn IdSource,
    ) -> Result<FrozenQuestion, UpstrokeError> {
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?;
        let task = self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?;
        let total_attempts: u32 = task
            .generations
            .iter()
            .map(|generation| generation.attempts)
            .sum();
        let rungs_spent = (task.rung as usize).saturating_add(1).max(1);
        let _ = attempts_on_rung;
        Ok(FrozenQuestion {
            id: ids.question_id(),
            key,
            kind,
            context: crate::engine::coordinator::question_context(
                crate::engine::coordinator::ParkSubject {
                    display_id: entry.display_id.as_str(),
                    title: &entry.spec.title,
                    acceptance: &entry.spec.acceptance,
                    attempts: total_attempts,
                    rungs_spent,
                },
                kind,
                failure,
            ),
            options: crate::engine::coordinator::question_options(kind),
        })
    }

    fn ladder_position(&self, key: TaskKey) -> Result<(u32, u32), UpstrokeError> {
        let task = self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?;
        Ok((task.rung, task.attempts_on_rung))
    }

    fn deferrals_recorded(&self, key: TaskKey) -> Result<u32, UpstrokeError> {
        Ok(self
            .handle
            .fold
            .task(key)
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's fold", key.index()),
            })?
            .defers)
    }

    fn ladder_policy(&self, key: TaskKey) -> Result<crate::ladder::LadderPolicy, UpstrokeError> {
        let entry = self
            .handle
            .fold
            .registry()
            .and_then(|registry| registry.get(key))
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!("task {} is not in this run's frozen registry", key.index()),
            })?;
        let limits = self
            .handle
            .fold
            .started()
            .ok_or_else(|| UpstrokeError::Refused {
                message: "the run has not started".to_owned(),
            })?
            .limits;
        Ok(crate::ladder::LadderPolicy {
            attempts_per: entry.ladder.attempts_per,
            rungs: entry.ladder.rungs.len(),
            max_defers: limits.max_defers,
        })
    }

    fn dispatch_request(
        &self,
        key: TaskKey,
        generation: GenerationId,
    ) -> Result<DispatchRequest, UpstrokeError> {
        let paths = self.handle.fold.predicted_region(key).ok_or_else(|| {
            UpstrokeError::Refused {
                message: format!(
                    "the fold selected task {} for dispatch and the frozen registry has no such \
                     entry; the two disagree and nothing is dispatched",
                    key.0
                ),
            }
        })?;
        Ok(DispatchRequest {
            key,
            generation,
            base: self.handle.started.base_sha.clone(),
            kind: DispatchKind::Ordinary { paths },
        })
    }

    fn emit(
        &mut self,
        body: TopologyEventBody,
        seams: &RunSeams<'_>,
        hooks: &mut dyn TopologyHooks,
    ) -> Result<(), UpstrokeError> {
        let mut emitter = RunEmitter {
            identity: &self.identity,
            state: EmitState {
                fold: &mut self.handle.fold,
                log: &mut self.handle.log,
                reservations: &mut self.reservations,
                warnings: &mut self.warnings,
            },
            clock: seams.clock,
        };
        emitter
            .emit(body, hooks)
            .map_err(|failure| failure.discharging(&mut self.invocations))
    }
}

#[cfg(test)]
mod tests;
