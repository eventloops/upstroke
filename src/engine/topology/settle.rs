//! Extended notes: `docs/internals/engine/topology/settle.md`

use std::time::Duration;

use crate::error::UpstrokeError;
use crate::events::{AttemptRecord, FailureRecord, RunOutcome};
use crate::interaction::{self, Sleeper};
use crate::ladder::Next;
use crate::topology::events::{
    AttemptFinished4, AttemptNumber, AttemptSettlement, AttemptStarted4, DeferWaitElapsed4, Epoch,
    FrozenQuestion, GenerationCloseReason, GenerationClosed, GenerationId, Materialization,
    RungBinding, SessionId, SettlementTransition,
};
use crate::topology::fold::{GenerationClass, GenerationFold, TopologyFold};
use crate::topology::registry::TaskKey;
use crate::workspace_manager::{EffectHooks, Quiescence, Slot, VerifyFailure, WorkspaceManager};

use super::identity::{ReservationKind, Reservations};

#[derive(Debug, Clone)]
pub struct FinishedAttempt {
    pub key: TaskKey,
    pub generation: GenerationId,
    pub attempt: AttemptNumber,
    pub record: AttemptRecord,
    pub next: Next,
    pub session: Option<SessionId>,
    pub question: Option<FrozenQuestion>,
    pub halts_run: bool,
    pub defers: u32,
    pub reason: String,
    pub rung: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settled {
    pub event: AttemptFinished4,
    pub spent_attempt: bool,
}

pub fn settle_failed(
    fold: &TopologyFold,
    finished: &FinishedAttempt,
) -> Result<Settled, UpstrokeError> {
    let epoch = fold
        .epoch()
        .ok_or_else(|| refused("the run has not started"))?;
    let generation = open_generation(fold, finished.key, finished.generation)?;

    if let Next::RetrySameRung { resume: true } = finished.next {
        if let Some(session) = &finished.session {
            return Ok(Settled {
                event: AttemptFinished4 {
                    key: finished.key,
                    generation: finished.generation,
                    attempt: finished.attempt,
                    record: Box::new(finished.record.clone()),
                    settlement: AttemptSettlement::Retained {
                        retained_session: session.clone(),
                        retained_incarnation: epoch,
                    },
                },
                spent_attempt: spent(finished),
            });
        }
    }

    let transition = match &finished.next {
        Next::RetrySameRung { .. } => SettlementTransition::Retry,
        Next::Escalate => SettlementTransition::Escalated {
            rung: finished.rung,
        },
        Next::Defer => SettlementTransition::Deferred {
            defers: finished.defers.saturating_add(1),
            reason: finished.reason.clone(),
        },
        Next::AskHuman(_) => SettlementTransition::Parked {
            question: finished
                .question
                .clone()
                .ok_or_else(|| refused("a parking settlement records the question it raised"))?,
        },
        Next::Fail => SettlementTransition::Failed {
            halts_run: finished.halts_run,
            reason: finished.reason.clone(),
        },
    };
    let lease = generation.lease.expected(false);
    Ok(Settled {
        event: AttemptFinished4 {
            key: finished.key,
            generation: finished.generation,
            attempt: finished.attempt,
            record: Box::new(finished.record.clone()),
            settlement: AttemptSettlement::Closed { transition, lease },
        },
        spent_attempt: spent(finished),
    })
}

fn spent(finished: &FinishedAttempt) -> bool {
    crate::ladder::spends_allowance(finished.record.failure.as_ref().map(FailureRecord::shape))
}

#[must_use]
pub fn rematerialize_question(finished: &AttemptFinished4) -> Option<&FrozenQuestion> {
    match &finished.settlement {
        AttemptSettlement::Closed {
            transition: SettlementTransition::Parked { question },
            ..
        } => Some(question),
        AttemptSettlement::Closed { .. } | AttemptSettlement::Retained { .. } => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Deferral {
    base: Duration,
    round: u32,
}

impl Deferral {
    #[must_use]
    pub const fn new(base: Duration) -> Self {
        Self { base, round: 0 }
    }

    #[must_use]
    pub const fn default_backoff() -> Self {
        Self::new(interaction::DEFAULT_DEFER_BACKOFF)
    }

    pub fn wait(&mut self, sleeper: &dyn Sleeper) -> DeferWaitElapsed4 {
        let waited = interaction::defer_backoff(self.base, self.round);
        sleeper.sleep(waited);
        self.round = self.round.saturating_add(1);
        DeferWaitElapsed4 {
            waited_ms: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            round: self.round,
        }
    }

    pub fn progressed(&mut self) {
        self.round = 0;
    }

    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }
}

pub fn close_generation(
    fold: &TopologyFold,
    key: TaskKey,
    reason: GenerationCloseReason,
) -> Result<GenerationClosed, UpstrokeError> {
    let generation = fold
        .task(key)
        .and_then(|task| {
            task.generations
                .iter()
                .find(|held| held.class != GenerationClass::Closed)
        })
        .ok_or_else(|| refused(&format!("task {key} has no open generation to close")))?;
    match generation.class {
        GenerationClass::OpenNoAttempt | GenerationClass::RetainedIdle { .. } => {}
        ref class => {
            return Err(refused(&format!(
                "generation {} of task {key} is {}, and a generation is closed only from \
                 open-with-no-attempt or retained-idle",
                generation.id.0,
                class_name(class)
            )));
        }
    }
    Ok(GenerationClosed {
        key,
        generation: generation.id,
        reason,
        lease: generation.lease.expected(false),
    })
}

pub fn close_retained(
    fold: &TopologyFold,
    reason: &GenerationCloseReason,
) -> Result<Vec<GenerationClosed>, UpstrokeError> {
    let mut closed = Vec::new();
    for key in retained_keys(fold) {
        closed.push(close_generation(fold, key, reason.clone())?);
    }
    Ok(closed)
}

#[must_use]
pub const fn run_ending(outcome: RunOutcome) -> GenerationCloseReason {
    GenerationCloseReason::RunEnding { outcome }
}

fn retained_keys(fold: &TopologyFold) -> Vec<TaskKey> {
    let len = fold.registry().map_or(0, |registry| {
        u32::try_from(registry.len()).unwrap_or(u32::MAX)
    });
    (0..len)
        .map(TaskKey)
        .filter(|key| {
            fold.task(*key).is_some_and(|task| {
                task.generations.iter().any(|generation| {
                    matches!(generation.class, GenerationClass::RetainedIdle { .. })
                })
            })
        })
        .collect()
}

pub trait WorktreeVerify {
    fn verify(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        expected: &Quiescence,
    ) -> Result<Result<(), VerifyFailure>, UpstrokeError>;
}

#[derive(Debug, Clone, Copy)]
pub struct ManagedWorktrees<'a>(&'a WorkspaceManager);

impl<'a> ManagedWorktrees<'a> {
    #[must_use]
    pub const fn new(manager: &'a WorkspaceManager) -> Self {
        Self(manager)
    }
}

impl WorktreeVerify for ManagedWorktrees<'_> {
    fn verify(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        expected: &Quiescence,
    ) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
        self.0.verify_worktree(hooks, slot, expected)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RetryOutcome {
    Start(Box<AttemptStarted4>),
    Close {
        closed: GenerationClosed,
        failure: VerifyFailure,
    },
}

#[derive(Debug, Clone)]
pub struct RetryRequest {
    pub key: TaskKey,
    pub slot: Slot,
    pub retained_tree: String,
    pub binding: RungBinding,
    pub rung: u32,
    pub pool: Option<String>,
    pub materialization: Option<Materialization>,
}

pub fn retry(
    fold: &TopologyFold,
    reservations: &mut Reservations,
    worktrees: &dyn WorktreeVerify,
    hooks: &mut dyn EffectHooks,
    request: &RetryRequest,
) -> Result<RetryOutcome, UpstrokeError> {
    let epoch = fold
        .epoch()
        .ok_or_else(|| refused("the run has not started"))?;
    let (generation, session, attempt) = retained(fold, request.key, epoch)?;

    reservations.take(request.key, ReservationKind::Retry)?;

    let verified = match worktrees.verify(
        hooks,
        &request.slot,
        &Quiescence::HoldsTree(request.retained_tree.clone()),
    ) {
        Ok(verified) => verified,
        Err(error) => {
            reservations.cancel(request.key, ReservationKind::Retry)?;
            return Err(error);
        }
    };

    if let Err(failure) = verified {
        reservations.cancel(request.key, ReservationKind::Retry)?;
        let closed = close_generation(fold, request.key, GenerationCloseReason::WorktreeMissing)?;
        return Ok(RetryOutcome::Close { closed, failure });
    }

    Ok(RetryOutcome::Start(Box::new(AttemptStarted4 {
        key: request.key,
        generation,
        attempt,
        rung: request.rung,
        binding: request.binding.clone(),
        pool: request.pool.clone(),
        resume_session: Some(session),
        materialization_observed: request.materialization,
    })))
}

fn retained(
    fold: &TopologyFold,
    key: TaskKey,
    epoch: Epoch,
) -> Result<(GenerationId, SessionId, AttemptNumber), UpstrokeError> {
    let generation = fold
        .task(key)
        .and_then(|task| {
            task.generations
                .iter()
                .find(|held| held.class != GenerationClass::Closed)
        })
        .ok_or_else(|| refused(&format!("task {key} has no open generation to retry")))?;
    let GenerationClass::RetainedIdle {
        session,
        incarnation,
    } = &generation.class
    else {
        return Err(refused(&format!(
            "generation {} of task {key} is {}, and only a retained-idle generation is retried \
             in place",
            generation.id.0,
            class_name(&generation.class)
        )));
    };
    if *incarnation != epoch {
        return Err(refused(&format!(
            "the session of generation {} of task {key} was retained by incarnation {} and this \
             run has resumed {} time(s): a retained session belongs to the incarnation that \
             retained it",
            generation.id.0, incarnation.0, epoch.0
        )));
    }
    Ok((
        generation.id,
        session.clone(),
        AttemptNumber(generation.attempts.saturating_add(1)),
    ))
}

fn open_generation(
    fold: &TopologyFold,
    key: TaskKey,
    generation: GenerationId,
) -> Result<&GenerationFold, UpstrokeError> {
    let open = fold
        .task(key)
        .and_then(|task| {
            task.generations
                .iter()
                .find(|held| held.class != GenerationClass::Closed)
        })
        .ok_or_else(|| refused(&format!("task {key} has no open generation")))?;
    if open.id != generation {
        return Err(refused(&format!(
            "this settlement names generation {} of task {key} and generation {} is the open one",
            generation.0, open.id.0
        )));
    }
    Ok(open)
}

fn class_name(class: &GenerationClass) -> &'static str {
    match class {
        GenerationClass::OpenNoAttempt => "open with no attempt",
        GenerationClass::InFlight { .. } => "in flight",
        GenerationClass::RetainedIdle { .. } => "retained idle",
        GenerationClass::Promoting => "promoting",
        GenerationClass::Closed => "closed",
    }
}

fn refused(message: &str) -> UpstrokeError {
    UpstrokeError::Refused {
        message: message.to_owned(),
    }
}

#[cfg(test)]
pub(crate) mod tests;
