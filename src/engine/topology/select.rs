//! Extended notes: `docs/internals/engine/topology/select.md`

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::events::{AttemptRecord, BudgetKind};
use crate::ir::QuestionId;
use crate::topology::events::{
    AttemptNumber, BudgetExceeded4, CandidateRef, DerivedOutcome, Epoch, GenerationId,
    TopologyEvent, TopologyEventBody,
};
use crate::topology::fold::{GenerationClass, TaskState, TopologyFold};
use crate::topology::registry::TaskKey;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spend {
    run: f64,
    per_task: BTreeMap<TaskKey, f64>,
}

impl Spend {
    pub const fn run_total(&self) -> f64 {
        self.run
    }

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, key: TaskKey, record: &AttemptRecord) {
        let attempt = record.cost_usd.unwrap_or(0.0) + record.review_cost_usd().unwrap_or(0.0);
        self.run += attempt;
        *self.per_task.entry(key).or_insert(0.0) += attempt;
    }

    #[must_use]
    pub fn replay(events: &[TopologyEvent]) -> Self {
        let mut spend = Self::new();
        for event in events {
            let (key, record) = match &event.body {
                TopologyEventBody::AttemptFinished { data } => (data.key, &*data.record),
                TopologyEventBody::CandidatePrepared { data } => (data.key, &*data.attempt),
                _ => continue,
            };
            spend.record(key, record);
        }
        spend
    }

    #[must_use]
    pub fn run_usd(&self) -> f64 {
        self.run
    }

    #[must_use]
    pub fn task_usd(&self, key: TaskKey) -> f64 {
        self.per_task.get(&key).copied().unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ceiling {
    pub run_usd: Option<f64>,
    pub task_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breach {
    pub budget: BudgetKind,
    pub limit_usd: f64,
    pub spent_usd: f64,
}

impl Ceiling {
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            run_usd: None,
            task_usd: None,
        }
    }

    #[must_use]
    pub fn breach(&self, spend: &Spend, key: TaskKey) -> Option<Breach> {
        self.run_breach(spend)
            .or_else(|| self.task_breach(spend, key))
    }

    fn run_breach(&self, spend: &Spend) -> Option<Breach> {
        let limit = self.run_usd?;
        let spent = spend.run_usd();
        (spent >= limit).then_some(Breach {
            budget: BudgetKind::Run,
            limit_usd: limit,
            spent_usd: spent,
        })
    }

    fn task_breach(&self, spend: &Spend, key: TaskKey) -> Option<Breach> {
        let limit = self.task_usd?;
        let spent = spend.task_usd(key);
        (spent >= limit).then_some(Breach {
            budget: BudgetKind::Task,
            limit_usd: limit,
            spent_usd: spent,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Poisoned,
    BudgetExceeded(Box<BudgetExceeded4>),
    Integrate {
        candidate: Box<CandidateRef>,
    },
    Retry {
        key: TaskKey,
        generation: GenerationId,
        attempt: AttemptNumber,
    },
    Dispatch {
        key: TaskKey,
        generation: GenerationId,
        continuing: bool,
    },
    Backoff,
    HardBlock {
        questions: Vec<QuestionId>,
    },
    Closure(DerivedOutcome),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Admitted {
    BudgetExceeded(Box<BudgetExceeded4>),
    Retry {
        key: TaskKey,
        generation: GenerationId,
        attempt: AttemptNumber,
    },
    Dispatch {
        key: TaskKey,
        generation: GenerationId,
        continuing: bool,
    },
    Backoff,
    HardBlock {
        questions: Vec<QuestionId>,
    },
}

#[must_use]
pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {
    if fold.is_poisoned() {
        return Step::Poisoned;
    }
    let Some(epoch) = fold.epoch() else {
        return Step::Closure(fold.derived_outcome());
    };

    if fold.run_is_ending() {
        return Step::Closure(fold.derived_outcome());
    }

    if let Some(candidate) = eligible_integration(fold) {
        return match ceiling.run_breach(spend) {
            Some(breach) => budget_exceeded(epoch, breach, None),
            None => Step::Integrate {
                candidate: Box::new(candidate),
            },
        };
    }
    if let Some((key, generation, attempt)) = first_ready_retry(fold) {
        return ceiling_or(ceiling, spend, epoch, key, || Step::Retry {
            key,
            generation,
            attempt,
        });
    }
    if let Some((key, generation, continuing)) = first_ready(fold) {
        return ceiling_or(ceiling, spend, epoch, key, || Step::Dispatch {
            key,
            continuing,
            generation,
        });
    }
    if backoff_pending(fold) {
        return Step::Backoff;
    }
    if fold.questions_open() {
        return Step::HardBlock {
            questions: open_questions(fold),
        };
    }
    Step::Closure(fold.derived_outcome())
}

pub fn checkpoint(step: Step) -> Result<Admitted, UpstrokeError> {
    match step {
        Step::BudgetExceeded(exceeded) => Ok(Admitted::BudgetExceeded(exceeded)),
        Step::Retry {
            key,
            generation,
            attempt,
        } => Ok(Admitted::Retry {
            key,
            generation,
            attempt,
        }),
        Step::Dispatch {
            key,
            generation,
            continuing,
        } => Ok(Admitted::Dispatch {
            key,
            generation,
            continuing,
        }),
        Step::Backoff => Ok(Admitted::Backoff),
        Step::HardBlock { questions } => Ok(Admitted::HardBlock { questions }),
        Step::Integrate { candidate } => Err(UpstrokeError::Refused {
            message: format!(
                "this build does not integrate: candidate {} of task {} is eligible, and \
                 `merge_prepared`, `merge_rejected` and `task_merged` are terminals it does not \
                 implement, so it refuses before appending the `merge_verification_started` that \
                 would start one",
                candidate.candidate_ref, candidate.key
            ),
        }),
        Step::Closure(outcome) => Err(UpstrokeError::Refused {
            message: format!(
                "this build does not end a run: closure derives {outcome:?}, and the terminal \
                 finalization `run_end_policy` attaches to `run_finished` is not implemented \
                 here, so it refuses before appending it"
            ),
        }),
        Step::Poisoned => Err(UpstrokeError::Refused {
            message: "an append returned an error and this process's fold is poisoned: nothing \
                      further is selected, and no report, cleanup or question payload is derived \
                      from it"
                .to_owned(),
        }),
    }
}

fn eligible_integration(fold: &TopologyFold) -> Option<CandidateRef> {
    if !fold.integration_admissible() {
        return None;
    }
    let queue = fold.queue()?;
    let leases = fold.leases()?;
    let policy = &fold.started()?.path_policy;
    queue
        .first_eligible(
            |key| fold.task_state(key) == Some(TaskState::AwaitingInput),
            leases,
            policy,
        )
        .map(|entry| entry.candidate.clone())
}

fn first_ready_retry(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, AttemptNumber)> {
    keys(fold).find_map(|key| {
        if !fold.ready_retry(key) {
            return None;
        }
        let generation = open_generation(fold, key)?;
        let task = fold.task(key)?;
        let open = task.generations.iter().find(|held| held.id == generation)?;
        matches!(open.class, GenerationClass::RetainedIdle { .. }).then(|| {
            (
                key,
                generation,
                AttemptNumber(open.attempts.saturating_add(1)),
            )
        })
    })
}

fn first_ready(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, bool)> {
    keys(fold).find_map(|key| {
        if let Some(generation) = fold.open_no_attempt(key) {
            return Some((key, generation, true));
        }
        if !fold.ready(key) {
            return None;
        }
        let task = fold.task(key)?;
        let generation = u32::try_from(task.generations.len()).ok()?;
        Some((key, GenerationId(generation), false))
    })
}

fn open_generation(fold: &TopologyFold, key: TaskKey) -> Option<GenerationId> {
    fold.task(key)?
        .generations
        .iter()
        .find(|generation| generation.class != GenerationClass::Closed)
        .map(|generation| generation.id)
}

fn keys(fold: &TopologyFold) -> impl Iterator<Item = TaskKey> + '_ {
    let len = fold.registry().map_or(0, |registry| {
        u32::try_from(registry.len()).unwrap_or(u32::MAX)
    });
    (0..len).map(TaskKey)
}

fn backoff_pending(fold: &TopologyFold) -> bool {
    !fold.run_is_ending() && fold.backoff_pending()
}

fn open_questions(fold: &TopologyFold) -> Vec<QuestionId> {
    fold.open_questions()
        .map(|questions| questions.keys().cloned().collect())
        .unwrap_or_default()
}

fn ceiling_or(
    ceiling: &Ceiling,
    spend: &Spend,
    epoch: Epoch,
    key: TaskKey,
    admitted: impl FnOnce() -> Step,
) -> Step {
    match ceiling.breach(spend, key) {
        Some(breach) => budget_exceeded(epoch, breach, Some(key)),
        None => admitted(),
    }
}

fn budget_exceeded(epoch: Epoch, breach: Breach, key: Option<TaskKey>) -> Step {
    Step::BudgetExceeded(Box::new(BudgetExceeded4 {
        epoch,
        budget: breach.budget,
        limit_usd: breach.limit_usd,
        spent_usd: breach.spent_usd,
        key,
    }))
}

#[cfg(test)]
mod tests;
