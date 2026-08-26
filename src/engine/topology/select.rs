//! Selection: the eligibility order, the ceiling, and the checkpoint refusals.
//!
//! `decisions.sequential_substrate.loop` is one sentence with six branches and
//! a fixed order, and `eligibility_order` states the part of that order this
//! module exists to keep: *"eligible integration precedes ready_retry precedes
//! new ordinary dispatch"*. The order is not a scheduling preference. An
//! integration that lost to a dispatch would let the queue grow behind a merge
//! entitlement nothing is using, and a fresh dispatch that beat a `ready_retry`
//! would abandon a retained session — and with it the cumulative tree the
//! retry exists to re-gate — for a generation that starts from nothing.
//!
//! # This module appends nothing
//!
//! [`select`] is a pure function of the fold, the ceiling and the reported
//! spend. It returns the branch the loop takes; the loop performs it. That
//! division is what makes the checkpoint refusal below expressible as a type
//! rather than as a rule a caller is asked to remember, and it is why the
//! ceiling is checked *here*: `loop` puts the check inside each admitting
//! branch, before the provisional reservation and before any effect, so a
//! breach has to be decided by whatever decides the branch.
//!
//! # The structural predicates are the fold's
//!
//! `decisions.admission_and_leases` defines `ready` and `ready_retry` as
//! "structural over fold state only" and the fold implements them.
//! [`crate::topology::fold::TopologyFold`] exposes `ready`, `ready_retry`,
//! `pipeline_reservable`, `structurally_admissible` and
//! `integration_admissible`, and every one of them is false once the fold is
//! poisoned. It exposes `run_is_ending`, `backoff_pending` and
//! `questions_open` beside them — statements about the run rather than
//! authorisations, which is why those three survive a poisoning and why
//! `derived_outcome` reads the same three. Nothing here re-derives any of
//! them: a second implementation of "which generation classes hold the
//! pipeline entitlement", or of "which tasks are waiting on a wait", is two
//! rules that can disagree, and `wrong_internal_assumption` is this project's
//! largest measured root cause by a factor of three.
//!
//! What is left for this module is exactly the packet's own division of
//! labour: **which** eligible item to take, and **whether the ceiling permits
//! it**. `CandidateQueue::first_eligible` answers the first of those for an
//! integration; ascending task key answers it for a dispatch and a retry,
//! which is §14's "lowest plan index first" over the dense registry keys.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::UpstrokeError;
use crate::events::{AttemptRecord, BudgetKind};
use crate::ir::QuestionId;
use crate::topology::events::{
    AttemptNumber, BudgetExceeded4, CandidateRef, DerivedOutcome, Epoch, GenerationId,
    TopologyEvent, TopologyEventBody,
};
use crate::topology::fold::{GenerationClass, TaskState, TopologyFold};
use crate::topology::registry::TaskKey;

// ---------------------------------------------------------------------------
// Reported spend
// ---------------------------------------------------------------------------

/// §13's reported spend, derived by replaying the log.
///
/// The ledger's own figure, with the ledger's own honesty: an attempt whose
/// route reported no dollars contributes nothing, so every number here is a
/// **floor** rather than a total. `BudgetExceeded4::spent_usd` documents the
/// same thing about the field this feeds, and `decisions` puts the ceiling
/// against *reported* dollars deliberately — a table of vendor rates inside a
/// shipped binary goes stale silently and flatters.
///
/// Derived rather than accumulated because INV-02 admits one reader: a live
/// run and a replay of the bytes it wrote must reach the same ceiling
/// decision, and a counter that only the live path incremented would not.
/// [`Self::replay`] is that reader and [`Self::record`] is the single step it
/// is made of, so the live loop that records each settlement as it appends it
/// reaches the identical value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spend {
    run: f64,
    per_task: BTreeMap<TaskKey, f64>,
}

impl Spend {
    /// The run's reported spend so far.
    ///
    /// A reader because the ceiling is not the only thing that needs it: a
    /// resumed process rebuilds this with [`Self::replay`], and the two totals
    /// have to be comparable or the comparison cannot be asserted.
    pub const fn run_total(&self) -> f64 {
        self.run
    }

    /// Nothing reported yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one attempt's reported dollars to the run and to its task.
    ///
    /// Worker and review spend together, exactly as the legacy ledger sums
    /// them: a review pass is spend the attempt caused, and a ceiling that
    /// counted only the implementer would let a two-reviewer route run past it.
    pub fn record(&mut self, key: TaskKey, record: &AttemptRecord) {
        let attempt = record.cost_usd.unwrap_or(0.0) + record.review_cost_usd().unwrap_or(0.0);
        self.run += attempt;
        *self.per_task.entry(key).or_insert(0.0) += attempt;
    }

    /// Reported spend over a whole log.
    ///
    /// Both event kinds that carry an [`AttemptRecord`] contribute: a failed
    /// attempt records one on `attempt_finished`, and a **successful** one
    /// records it on `candidate_prepared` — `candidate_prepared` is the sole
    /// successful attempt settlement (INV-07), so a reader that only walked
    /// `attempt_finished` would price a run at the cost of its failures.
    #[must_use]
    pub fn replay(events: &[TopologyEvent]) -> Self {
        let mut spend = Self::new();
        // **One attempt, one contribution.** Both event kinds carry an
        // `AttemptRecord`, and for a *successful* attempt both are appended:
        // `attempt_finished{Succeeded}` moves the generation to `Promoting`,
        // and `candidate_prepared` records the candidate that settlement
        // authorized. Counting each occurrence would price every successful
        // attempt twice — and only on replay, because a live run records it
        // once. A live total and a replay of that run's own log would then
        // disagree, which is the ground truth this project measures everything
        // else against.
        //
        // Keyed by the attempt's identity rather than by event kind, so a
        // vocabulary that later carries the record in a third place is counted
        // once too.
        let mut counted: BTreeSet<(TaskKey, u32, u32)> = BTreeSet::new();
        for event in events {
            let (key, generation, attempt, record) = match &event.body {
                TopologyEventBody::AttemptFinished { data } => {
                    (data.key, data.generation.0, data.attempt.0, &*data.record)
                }
                TopologyEventBody::CandidatePrepared { data } => (
                    data.key,
                    data.generation.0,
                    data.attempt.attempt,
                    &*data.attempt,
                ),
                _ => continue,
            };
            if counted.insert((key, generation, attempt)) {
                spend.record(key, record);
            }
        }
        spend
    }

    /// Reported spend across the whole run.
    #[must_use]
    pub fn run_usd(&self) -> f64 {
        self.run
    }

    /// Reported spend attributed to one task.
    #[must_use]
    pub fn task_usd(&self, key: TaskKey) -> f64 {
        self.per_task.get(&key).copied().unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// The ceiling
// ---------------------------------------------------------------------------

/// The run's frozen spend ceilings.
///
/// A value rather than a trait: there is one rule, it is arithmetic over two
/// optional limits, and a seam here would only let a test disagree with
/// production about which limit is stricter.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ceiling {
    /// The whole run's ceiling, when the operator set one.
    pub run_usd: Option<f64>,
    /// One task's ceiling, when the operator set one.
    pub task_usd: Option<f64>,
}

/// A ceiling that refused the next spawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breach {
    /// Which ceiling.
    pub budget: BudgetKind,
    /// The limit it names.
    pub limit_usd: f64,
    /// Reported spend against it — a floor. See [`Spend`].
    pub spent_usd: f64,
}

impl Ceiling {
    /// No ceilings configured, which never breaches.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            run_usd: None,
            task_usd: None,
        }
    }

    /// The breach that refuses the next spawn for `key`, if there is one.
    ///
    /// `run_usd` is checked before `task_usd` because it is the stricter
    /// claim: a run at its overall ceiling is done whatever any individual
    /// task has spent, and naming the run budget is what tells the operator
    /// which number to raise.
    #[must_use]
    pub fn breach(&self, spend: &Spend, key: TaskKey) -> Option<Breach> {
        self.run_breach(spend)
            .or_else(|| self.task_breach(spend, key))
    }

    /// The run ceiling alone.
    ///
    /// Split out because one branch checks this and not [`Self::breach`]: an
    /// integration spawns no worker and is charged to no task. See
    /// [`select`]'s integration branch for why that is not an omission.
    fn run_breach(&self, spend: &Spend) -> Option<Breach> {
        let limit = self.run_usd?;
        let spent = spend.run_usd();
        // `>=` rather than `>`: the ceiling refuses the *next* spawn, so
        // reaching it is already a refusal.
        (spent >= limit).then_some(Breach {
            budget: BudgetKind::Run,
            limit_usd: limit,
            spent_usd: spent,
        })
    }

    /// One task's ceiling alone, on the same `>=` boundary as the run's.
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

// ---------------------------------------------------------------------------
// The branch
// ---------------------------------------------------------------------------

/// The branch of `sequential_substrate.loop` this state selects.
///
/// One variant per branch of the packet sentence, in the packet's order, so
/// that "the order is fixed" is a property a reader can check against the
/// source rather than reconstruct from control flow.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// An append returned an error and this process's fold is poisoned.
    ///
    /// Not a branch of the loop — the append-error protocol has already ended
    /// the command. It is here because a predicate that answered `false` and a
    /// selector that then chose *closure* would turn "no further transition"
    /// into "end the run", which is a durable decision derived from a state
    /// this process cannot vouch for.
    Poisoned,
    /// The ceiling refused the next spawn. Append this **before any effect**,
    /// then proceed to closure.
    BudgetExceeded(Box<BudgetExceeded4>),
    /// An eligible integration. Take the `{pipeline, merge}` reservation and
    /// integrate exactly one.
    Integrate {
        /// The candidate `CandidateQueue::first_eligible` chose.
        candidate: Box<CandidateRef>,
    },
    /// A `ready_retry` task. Take the `{pipeline}` reservation and run the
    /// next attempt in the retained generation.
    Retry {
        /// The task.
        key: TaskKey,
        /// Its open, retained generation.
        generation: GenerationId,
        /// The attempt number the retry starts: the generation's highest plus
        /// one, which is what `check_attempt_started` requires.
        attempt: AttemptNumber,
    },
    /// A `ready` task. Take the dispatch reservation and dispatch.
    Dispatch {
        /// The task.
        key: TaskKey,
        /// The generation this dispatch opens: dense, so the count of the
        /// task's generations so far.
        generation: GenerationId,
        /// Whether that generation **already exists**, open with no attempt.
        ///
        /// `T-DISPATCH`'s resume action is "continue attempt (no spend
        /// repeats)": a run killed between `task_dispatched` and
        /// `attempt_started` leaves the generation `OpenNoAttempt`, recovery
        /// step (g) verifies or recreates its worktree, and the loop starts the
        /// attempt in it. `task_dispatched` is already durable, so the branch
        /// reuses rather than appending a second one.
        ///
        /// Not a new branch: `eligibility_order` names "new ordinary dispatch",
        /// and a continuation is not a new one. It is the same branch reaching
        /// the same attempt over ground that already exists.
        continuing: bool,
    },
    /// Deferred work and nothing else. Sleep the defer backoff, then append
    /// `defer_wait_elapsed`.
    Backoff,
    /// Open questions and nothing else. Apply the hard-block rules.
    HardBlock {
        /// The questions blocking, in id order.
        questions: Vec<QuestionId>,
    },
    /// Run-end closure is due, with the outcome the fold derives.
    Closure(DerivedOutcome),
}

/// The branches an **intermediate build** is entitled to perform.
///
/// [`Step`] has seven variants and this has five. The two that are missing are
/// the whole of `checkpoint_refusals` for PR7: there is no value of this type
/// that can carry an integration or a run end, so no caller holding one can
/// append `merge_verification_started` or `run_finished`. That is the refusal
/// made unrepresentable rather than remembered.
#[derive(Debug, Clone, PartialEq)]
pub enum Admitted {
    /// [`Step::BudgetExceeded`].
    BudgetExceeded(Box<BudgetExceeded4>),
    /// [`Step::Retry`].
    Retry {
        /// The task.
        key: TaskKey,
        /// Its open, retained generation.
        generation: GenerationId,
        /// The attempt number the retry starts.
        attempt: AttemptNumber,
    },
    /// [`Step::Dispatch`].
    Dispatch {
        /// The task.
        key: TaskKey,
        /// The generation this dispatch opens, or already opened.
        generation: GenerationId,
        /// Whether the generation already exists. See [`Step::Dispatch`].
        continuing: bool,
    },
    /// [`Step::Backoff`].
    Backoff,
    /// [`Step::HardBlock`].
    HardBlock {
        /// The questions blocking, in id order.
        questions: Vec<QuestionId>,
    },
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

/// The branch this state selects, in `eligibility_order`.
///
/// Appends nothing and performs nothing. The ceiling is consulted only inside
/// an admitting branch, which is where `loop` puts it: a run with no
/// admissible work never asks the ceiling anything, because there is no spawn
/// for it to refuse and `budget_exceeded` is a record *of a refusal*.
#[must_use]
pub fn select(fold: &TopologyFold, ceiling: &Ceiling, spend: &Spend) -> Step {
    if fold.is_poisoned() {
        return Step::Poisoned;
    }
    let Some(epoch) = fold.epoch() else {
        // No `run_started`: nothing has been recorded, so nothing is
        // selectable and nothing has ended.
        return Step::Closure(fold.derived_outcome());
    };

    // **An ending run offers no work, whatever else is live.**
    //
    // `loop` says a breach "appends `budget_exceeded` before any effect and
    // **proceeds to closure**", and a halted run is the same shape one cause
    // over. `run_is_ending()` is `halted_at.is_some() || budget_stop_is_current()`.
    //
    // **One guard, at the top, rather than one per arm** — and that placement is
    // the repair rather than an implementation detail. Three of the eligibility
    // predicates already embed `!run_is_ending()` inside themselves
    // (`ready`, `ready_retry`, `integration_admissible`) and the fourth,
    // `open_no_attempt`, does not: it is a *statement* accessor whose doc
    // correctly declines to consult run state, and recovery step (g) depends on
    // that — (g) runs before `run_resumed` increments the epoch, so a
    // budget-stopped run whose reader refused would silently skip rebuilding its
    // worktrees. Patching the one arm would leave the next arm to be written in
    // the same position as `open_no_attempt` was.
    //
    // Found by round 3's `loop` lens, measured end to end: five consecutive
    // `step()` calls each returned `Progress::BudgetExceeded` and appended a
    // duplicate stop record, because the continuation was offered, refused by
    // the ceiling, and offered again — a run that never terminates. With
    // `halted_at` set the same path returned `Dispatch { continuing: true }`: a
    // halted run spawning a worker.
    if fold.run_is_ending() {
        return Step::Closure(fold.derived_outcome());
    }

    if let Some(candidate) = eligible_integration(fold) {
        // The **run** ceiling, and not the task's. `BudgetExceeded4::key` is
        // "the task whose next attempt was refused. Not a failed task:
        // nothing judged it and nothing was spent on it", and an integration
        // is neither half of that sentence: it is not that task's next
        // attempt, and money *was* spent on it — the candidate exists because
        // an attempt succeeded and was paid for. Charging the task ceiling
        // here would refuse the *merge* of work already bought, permanently:
        // the candidate can never integrate and the task can never unspend.
        // An integration also spawns no worker, and the identities its
        // verification passes carry are `(sequence, role, ordinal)` rather
        // than a task's. The run ceiling still binds, because `loop` puts the
        // check inside every admitting branch and a run at its overall
        // ceiling is done whatever the branch would have been.
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

/// `checkpoint_refusals`: "an intermediate build refuses, **before any
/// append**, any operation whose terminals it does not implement".
///
/// PR7 appends `attempt_started` and implements its terminals; it does not
/// implement `merge_prepared`, `merge_rejected` or `task_merged`, so it never
/// appends `merge_verification_started` — INV-07's "every checkpoint build
/// implements every terminal reachable from any start it appends" is that
/// sentence read from the other end. Run-end closure is refused for the same
/// reason: `run_finished` is a terminal whose finalization PR7 does not
/// perform.
///
/// The refusal is taken on the [`Step`], which is a value nothing has acted
/// on: `select` performed no effect and appended nothing, so "before any
/// append" holds by construction rather than by the caller checking early
/// enough.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] naming the operation and the terminals PR7 does
/// not implement.
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

/// The candidate an eligible integration would take, if one is eligible.
///
/// `integration_admissible` decides *whether* — it is the fold's, it already
/// folds in "no unresolved transaction" and "run not ending", and it is false
/// on a poisoned fold. `first_eligible` decides *which*, over the same three
/// inputs the fold hands it.
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

/// The lowest-keyed `ready_retry` task, its retained generation, and the
/// attempt number the retry starts.
fn first_ready_retry(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, AttemptNumber)> {
    keys(fold).find_map(|key| {
        if !fold.ready_retry(key) {
            return None;
        }
        let generation = open_generation(fold, key)?;
        // Only a `RetainedIdle` generation is ever `ready_retry`; asking the
        // class here is not a second predicate but the way this function gets
        // the number without inventing it.
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

/// The lowest-keyed `ready` task and the generation its dispatch opens.
fn first_ready(fold: &TopologyFold) -> Option<(TaskKey, GenerationId, bool)> {
    keys(fold).find_map(|key| {
        // **A continuation first, and it cannot compete with a fresh dispatch.**
        // `T-DISPATCH`'s `authoritative_state` is "generation open
        // (OpenNoAttempt) ... entitlement derived from the open generation", so
        // an open generation holds the run's only entitlement at
        // `max_parallel = 1` and `ready` is false for every other task —
        // `pipeline_reservable` sees none free. The order between the two
        // therefore cannot arise in this build.
        //
        // **`eligibility_order` is silent on it**, naming only "eligible
        // integration precedes ready_retry precedes new ordinary dispatch",
        // and a continuation is not a *new* dispatch. Reported as a candidate
        // erratum rather than chosen here: at a wider pipeline the two can
        // coexist and the packet will have to say which wins.
        if let Some(generation) = fold.open_no_attempt(key) {
            return Some((key, generation, true));
        }
        if !fold.ready(key) {
            return None;
        }
        // refusals[10]: generations are dense per task, so the next one is the
        // count of the ones recorded.
        let task = fold.task(key)?;
        let generation = u32::try_from(task.generations.len()).ok()?;
        Some((key, GenerationId(generation), false))
    })
}

/// The open generation of `key`, if it has one.
fn open_generation(fold: &TopologyFold, key: TaskKey) -> Option<GenerationId> {
    fold.task(key)?
        .generations
        .iter()
        .find(|generation| generation.class != GenerationClass::Closed)
        .map(|generation| generation.id)
}

/// Every registered key, ascending.
fn keys(fold: &TopologyFold) -> impl Iterator<Item = TaskKey> + '_ {
    let len = fold.registry().map_or(0, |registry| {
        u32::try_from(registry.len()).unwrap_or(u32::MAX)
    });
    (0..len).map(TaskKey)
}

/// Whether the backoff branch is entered: something is waiting on a wait, and
/// neither `halted_at` nor this epoch's `budget_stop` is set.
///
/// `refusals[18]` refuses `defer_wait_elapsed` under either, and `loop` states
/// the same guard on the branch — so a selector that offered the branch anyway
/// would hand the loop an append the fold is about to refuse.
///
/// Both halves are the fold's. This function is the **conjunction** and
/// nothing else: an earlier version walked `0..registry.len()` for a
/// `Deferred` task while `TopologyFold::backoff_pending` walked its own
/// `tasks`, and `derived_outcome` reads the fold's. Two rules that can
/// disagree is precisely what this module's header argues against.
fn backoff_pending(fold: &TopologyFold) -> bool {
    !fold.run_is_ending() && fold.backoff_pending()
}

/// The open question ids, in id order.
///
/// Whether there are any is [`TopologyFold::questions_open`]; this builds the
/// payload the branch carries and decides nothing.
fn open_questions(fold: &TopologyFold) -> Vec<QuestionId> {
    fold.open_questions()
        .map(|questions| questions.keys().cloned().collect())
        .unwrap_or_default()
}

/// The ceiling check every admitting branch performs, and what it produces.
///
/// A breach is [`Step::BudgetExceeded`] carrying the exact event to append;
/// `loop` says it is appended "before any effect and proceeds to closure", so
/// the value the loop is handed is the event and not a flag it has to build
/// one from.
fn ceiling_or(
    ceiling: &Ceiling,
    spend: &Spend,
    epoch: Epoch,
    key: TaskKey,
    admitted: impl FnOnce() -> Step,
) -> Step {
    match ceiling.breach(spend, key) {
        // "The task whose next attempt was refused. Not a failed task:
        // nothing judged it and nothing was spent on it."
        Some(breach) => budget_exceeded(epoch, breach, Some(key)),
        None => admitted(),
    }
}

/// The stop a breach records.
///
/// `key` is `None` where no task's next attempt was refused, which is the
/// integration branch and only it.
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
mod tests {
    use super::*;
    use crate::events::RunOutcome;
    use crate::ladder::Next;
    use crate::topology::events::{
        AttemptSettlement, CandidateLeaseEffect, CandidatePrepared, GitRef, LeaseDisposition,
        RunStarted4, SettlementTransition, TaskCandidateCreated, TopologyLimits,
    };
    use crate::topology::fold::TopologyFold;

    use super::super::settle;
    use super::super::settle::tests::{
        ALEPH, BET, GIMEL, apply, dispatch, ev, finished, in_flight, inputs, label, question_for,
        record, region, resume_event, retained_generation, settle_into, sha, started,
    };

    // -----------------------------------------------------------------------
    // Fixtures the settlement lane does not need
    // -----------------------------------------------------------------------

    fn candidate_of(key: TaskKey, generation: u32) -> CandidateRef {
        CandidateRef {
            key,
            generation: GenerationId(generation),
            commit_sha: sha(&format!("commit-{}", label(key))),
            candidate_ref: GitRef(format!(
                "refs/upstroke/select/candidates/{}/{generation}",
                label(key)
            )),
        }
    }

    /// Take `key` all the way to a queued candidate: dispatch, attempt,
    /// success, prepare, create.
    fn queue_candidate(fold: &mut TopologyFold, key: TaskKey, generation: u32) -> CandidateRef {
        in_flight(fold, key, generation);
        // Through the settlement module, at the point `T-CAND-OBJ` puts it:
        // between the pin and `candidate_prepared`. A fixture that hand-built
        // this event would agree with itself about a shape neither half had
        // asked the module under test.
        let settled = settle::settle_succeeded(
            fold,
            key,
            GenerationId(generation),
            AttemptNumber(1),
            &record(1, Some(0.25)),
        )
        .expect("a succeeding attempt settles");
        assert_eq!(
            settled.settlement,
            AttemptSettlement::Closed {
                transition: SettlementTransition::Succeeded,
                // A generation that survives its settlement keeps its region
                // and hands it to the candidate at `candidate_prepared`.
                lease: LeaseDisposition::PredictedRetained,
            }
        );
        apply(
            fold,
            &ev(TopologyEventBody::AttemptFinished {
                data: Box::new(settled),
            }),
        );
        let candidate = candidate_of(key, generation);
        apply(
            fold,
            &ev(TopologyEventBody::CandidatePrepared {
                data: Box::new(CandidatePrepared {
                    key,
                    generation: GenerationId(generation),
                    attempt: Box::new(record(1, Some(0.25))),
                    base_sha: sha("base"),
                    parent_sha: sha("base"),
                    tree_sha: sha(&format!("tree-{}", label(key))),
                    commit_sha: candidate.commit_sha.clone(),
                    message: format!("{}: select candidate", label(key)),
                    prepared_ref: GitRef(format!("refs/upstroke/select/prepared/{}", label(key))),
                    candidate_ref: candidate.candidate_ref.clone(),
                    actual_paths: region(key),
                    lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region(key) },
                }),
            }),
        );
        apply(
            fold,
            &ev(TopologyEventBody::TaskCandidateCreated {
                data: TaskCandidateCreated {
                    candidate: candidate.clone(),
                },
            }),
        );
        candidate
    }

    /// Every task terminal and nothing queued: the state a run ends from.
    fn all_failed() -> TopologyFold {
        let mut fold = started();
        for key in [ALEPH, BET, GIMEL] {
            in_flight(&mut fold, key, 0);
            settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
        }
        fold
    }

    /// `started()` at a stated pipeline width.
    ///
    /// Every other selection fixture runs at `max_parallel = 3`, and the
    /// comment on that number is right about why: a test that ordered an
    /// integration ahead of a dispatch because the *entitlement* excluded the
    /// dispatch would prove nothing about `eligibility_order`. But 3 is a
    /// width `config` refuses to create a run at — `DEFAULT_MAX_PARALLEL` is 1
    /// and `[engine] max_parallel` above it is rejected outright — so a suite
    /// with no fixture below 3 never binds the entitlement clause of any
    /// predicate, and never asks what selection does at the only width
    /// production runs.
    fn started_at_width(max_parallel: u32) -> TopologyFold {
        let base = settle::tests::run_started();
        let limits = TopologyLimits {
            max_parallel,
            ..base.limits
        };
        let mut fold = TopologyFold::new(inputs());
        apply(
            &mut fold,
            &ev(TopologyEventBody::RunStarted {
                data: Box::new(RunStarted4 { limits, ..base }),
            }),
        );
        fold
    }

    fn no_spend() -> Spend {
        Spend::new()
    }

    fn review_costing(cost: Option<f64>) -> crate::events::ReviewRecord {
        crate::events::ReviewRecord {
            pass: "review".to_owned(),
            agent: "aleph-Frontier-agent".to_owned(),
            model: "aleph-Frontier-model".to_owned(),
            adapter: None,
            preflight_cli_version: None,
            effort: None,
            pool: None,
            cost_usd: cost,
            outcome: crate::events::ReviewPassOutcome::Passed,
        }
    }

    // -----------------------------------------------------------------------
    // eligibility_order
    // -----------------------------------------------------------------------

    /// "eligible integration precedes ready_retry precedes new ordinary
    /// dispatch".
    ///
    /// Three states that differ by exactly one removed alternative, so each
    /// assertion is about the branch that *lost*: in the first, a retry and a
    /// dispatch were both live and the integration still won; in the second, a
    /// dispatch was live and the retry still won.
    #[test]
    fn an_eligible_integration_precedes_a_retry_precedes_a_dispatch() {
        let mut fold = started();
        let candidate = queue_candidate(&mut fold, GIMEL, 0);
        retained_generation(&mut fold, BET, 0);

        assert!(fold.ready_retry(BET), "the retry alternative is not live");
        assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
        assert!(fold.integration_admissible());
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Integrate {
                candidate: Box::new(candidate)
            }
        );

        // Without the candidate, the retry wins over the dispatch.
        let mut fold = started();
        retained_generation(&mut fold, BET, 0);
        assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Retry {
                key: BET,
                generation: GenerationId(0),
                // The retry runs the *next* attempt of the generation that
                // retained the session, not a first attempt of a new one.
                attempt: AttemptNumber(2),
            }
        );

        // Without either, the dispatch. Lowest key first.
        let fold = started();
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Dispatch {
                key: ALEPH,
                generation: GenerationId(0),
                continuing: false,
            }
        );
    }

    /// At the width production runs, the entitlement decides every branch.
    ///
    /// `DEFAULT_MAX_PARALLEL` is 1 and `[engine] max_parallel` above it is
    /// refused for a fresh run, so one held entitlement is a full pipeline. An
    /// `OpenNoAttempt` generation is what a crash between `task_dispatched`
    /// and `attempt_started` leaves holding it, and recovery does not close it
    /// — so this is the state the resumed loop's first `select` sees.
    #[test]
    fn nothing_is_selected_at_width_one_while_the_single_entitlement_is_held() {
        let mut narrow = started_at_width(1);
        let candidate = queue_candidate(&mut narrow, GIMEL, 0);
        assert_eq!(
            select(&narrow, &Ceiling::unlimited(), &no_spend()),
            Step::Integrate {
                candidate: Box::new(candidate.clone())
            },
            "an eligible candidate with the slot free is selected"
        );

        apply(&mut narrow, &dispatch(ALEPH, 0));
        assert_eq!(narrow.pipeline_held(), 1);
        assert!(!narrow.pipeline_reservable(), "one of one");

        // **The entitlement's holder is the one thing still selectable**, and
        // that is `T-DISPATCH`'s "continue attempt (no spend repeats)": this
        // dispatch opened a generation and started no attempt, so the loop's
        // job is to start one in it. What the held entitlement forbids is a
        // *second* claim on it — the queued integration above is no longer
        // selected, which is what this test is measuring.
        assert_eq!(
            select(&narrow, &Ceiling::unlimited(), &no_spend()),
            Step::Dispatch {
                key: ALEPH,
                generation: GenerationId(0),
                continuing: true,
            },
            "selection spent the entitlement a second time, or lost the \
             generation it already opened"
        );

        // One slot wider, the identical state selects the integration: what
        // this asserts is the count, not something else about the fixture.
        let mut wider = started_at_width(2);
        let candidate = queue_candidate(&mut wider, GIMEL, 0);
        apply(&mut wider, &dispatch(ALEPH, 0));
        assert_eq!(wider.pipeline_held(), 1);
        assert_eq!(
            select(&wider, &Ceiling::unlimited(), &no_spend()),
            Step::Integrate {
                candidate: Box::new(candidate)
            }
        );
    }

    /// A dispatch opens the next dense generation, not generation zero again.
    #[test]
    fn a_dispatch_opens_the_next_dense_generation() {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        settle_into(
            &mut fold,
            &finished(ALEPH, 0, 1, Next::RetrySameRung { resume: false }),
        );
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Dispatch {
                key: ALEPH,
                generation: GenerationId(1),
                continuing: false,
            }
        );
    }

    /// The candidate the queue chooses, not the head of the queue.
    ///
    /// `first_eligible` skips an entry whose task is awaiting input rather
    /// than blocking behind it, and this is the selector inheriting that
    /// rather than re-deriving it.
    #[test]
    fn selection_takes_the_first_eligible_candidate_and_not_the_head() {
        let mut fold = started();
        let blocked = queue_candidate(&mut fold, ALEPH, 0);
        let free = queue_candidate(&mut fold, BET, 0);
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Integrate {
                candidate: Box::new(blocked)
            },
            "the queue is FIFO while every entry is eligible"
        );

        // Park ALEPH's task: its candidate keeps its place and loses its turn.
        apply(
            &mut fold,
            &ev(TopologyEventBody::QuestionRaised {
                data: crate::topology::events::QuestionRaised4 {
                    question: question_for(ALEPH),
                },
            }),
        );
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Integrate {
                candidate: Box::new(free)
            }
        );
    }

    // -----------------------------------------------------------------------
    // The ceiling
    // -----------------------------------------------------------------------

    /// Reported spend is derived by replaying the log, and both event kinds
    /// that carry a record contribute.
    #[test]
    fn reported_spend_replays_both_record_carrying_events() {
        let mut fold = started();
        let mut log = Vec::new();
        let started_event = ev(TopologyEventBody::RunStarted {
            data: Box::new(super::super::settle::tests::run_started()),
        });
        log.push(started_event);

        // A failure records on `attempt_finished`.
        in_flight(&mut fold, ALEPH, 0);
        let mut failing = finished(ALEPH, 0, 1, Next::Fail);
        failing.record = record(1, Some(0.75));
        let event = settle_into(&mut fold, &failing);
        log.push(ev(TopologyEventBody::AttemptFinished {
            data: Box::new(event),
        }));

        let spend = Spend::replay(&log);
        assert!((spend.run_usd() - 0.75).abs() < f64::EPSILON);
        assert!((spend.task_usd(ALEPH) - 0.75).abs() < f64::EPSILON);
        assert!(
            (spend.task_usd(BET)).abs() < f64::EPSILON,
            "spend leaked onto a task that never ran"
        );

        // A success records on `candidate_prepared`, and a replay that only
        // walked settlements would price the run at the cost of its failures.
        let mut fold = started();
        queue_candidate(&mut fold, BET, 0);
        let queued: Vec<TopologyEvent> = vec![ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(CandidatePrepared {
                key: BET,
                generation: GenerationId(0),
                attempt: Box::new(record(1, Some(0.25))),
                base_sha: sha("base"),
                parent_sha: sha("base"),
                tree_sha: sha("tree-bet"),
                commit_sha: candidate_of(BET, 0).commit_sha,
                message: "bet".to_owned(),
                prepared_ref: GitRef("refs/upstroke/select/prepared/bet".to_owned()),
                candidate_ref: candidate_of(BET, 0).candidate_ref,
                actual_paths: region(BET),
                lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region(BET) },
            }),
        })];
        let spend = Spend::replay(&queued);
        assert!((spend.run_usd() - 0.25).abs() < f64::EPSILON);

        // An unpriced route contributes nothing, which is why the number is a
        // floor and the field says so.
        let mut floored = Spend::new();
        floored.record(GIMEL, &record(1, None));
        assert!(floored.run_usd().abs() < f64::EPSILON);

        // Review spend counts too: a ceiling that priced only the implementer
        // would let a two-reviewer route run past it. The worker's dollars and
        // the two passes' are three different numbers so a sum that dropped
        // one lands somewhere this fixture does not hold.
        let mut reviewed = record(1, Some(0.125));
        reviewed.reviews = vec![
            review_costing(Some(0.25)),
            review_costing(Some(0.5)),
            // An unpriced pass, which contributes nothing and makes the total
            // a floor rather than a figure.
            review_costing(None),
        ];
        let mut with_reviews = Spend::new();
        with_reviews.record(ALEPH, &reviewed);
        assert!(
            (with_reviews.run_usd() - 0.875).abs() < f64::EPSILON,
            "review spend was dropped or double counted: {}",
            with_reviews.run_usd()
        );
        assert!((with_reviews.task_usd(ALEPH) - 0.875).abs() < f64::EPSILON);
    }

    /// **The selector's ceiling arm checks both budgets, not one.**
    ///
    /// `the_run_ceiling_is_checked_before_the_task_ceiling` exercises
    /// `Ceiling::breach` directly and is green for either half alone. The arm
    /// is what the loop actually runs, and catalogue entries `PR7-SELECT-020`
    /// and `PR7-SELECT-023` reduced `ceiling_or`'s call to
    /// `ceiling.task_breach(..)` and `ceiling.run_breach(..)` respectively —
    /// each dropping one comparison — and the whole suite stayed green **twice**.
    ///
    /// The two halves need opposite fixtures, and that is the whole reason
    /// neither was caught: a ceiling where both budgets are breached, or
    /// neither, cannot tell the halves apart. Each case below has **headroom in
    /// one budget and a breach in the other**.
    #[test]
    fn the_ceiling_arm_refuses_on_either_budget_alone() {
        // Case one: the task is over, the run has room. A dropped
        // `task_breach` admits an attempt the task's own budget refuses.
        let fold = started();
        assert!(fold.ready(ALEPH), "the dispatch alternative is not live");
        let mut spend = Spend::new();
        spend.record(ALEPH, &record(1, Some(0.6)));
        let only_task = Ceiling {
            run_usd: Some(10.0),
            task_usd: Some(0.5),
        };
        assert_eq!(
            only_task.run_breach(&spend),
            None,
            "the run budget must have headroom, or this case cannot tell a \
             dropped task comparison from a kept one"
        );
        match select(&fold, &only_task, &spend) {
            Step::BudgetExceeded(exceeded) => assert_eq!(
                exceeded.budget,
                BudgetKind::Task,
                "the arm named the wrong budget"
            ),
            other => panic!(
                "a task over its own ceiling was admitted because the run had \
                 room: {other:?}"
            ),
        }

        // Case two, the mirror: the run is over, this task has spent nothing.
        let fold = started();
        let mut spend = Spend::new();
        spend.record(BET, &record(1, Some(2.0)));
        let only_run = Ceiling {
            run_usd: Some(1.0),
            task_usd: Some(10.0),
        };
        assert_eq!(
            only_run.task_breach(&spend, ALEPH),
            None,
            "the selected task must have headroom, or this case cannot tell a \
             dropped run comparison from a kept one"
        );
        match select(&fold, &only_run, &spend) {
            Step::BudgetExceeded(exceeded) => assert_eq!(
                exceeded.budget,
                BudgetKind::Run,
                "the arm named the wrong budget"
            ),
            other => panic!(
                "a run over its ceiling dispatched a task that had spent \
                 nothing: {other:?}"
            ),
        }
    }

    /// The run ceiling is named before the task ceiling, and reaching a
    /// ceiling is already a refusal.
    #[test]
    fn the_run_ceiling_is_checked_before_the_task_ceiling() {
        let ceiling = Ceiling {
            run_usd: Some(1.0),
            task_usd: Some(0.5),
        };
        let mut spend = Spend::new();
        spend.record(ALEPH, &record(1, Some(0.6)));

        // Over the task ceiling, under the run ceiling.
        assert_eq!(
            ceiling.breach(&spend, ALEPH).map(|breach| breach.budget),
            Some(BudgetKind::Task)
        );
        assert_eq!(ceiling.breach(&spend, BET), None);

        // Over both: the run ceiling is the stricter claim and is what the
        // operator is told to raise.
        spend.record(BET, &record(1, Some(0.5)));
        let breach = ceiling.breach(&spend, ALEPH).expect("over the run ceiling");
        assert_eq!(breach.budget, BudgetKind::Run);
        assert!((breach.limit_usd - 1.0).abs() < f64::EPSILON);
        assert!((breach.spent_usd - 1.1).abs() < f64::EPSILON);

        // Exactly at the ceiling refuses the next spawn.
        let mut exact = Spend::new();
        exact.record(ALEPH, &record(1, Some(1.0)));
        assert_eq!(
            ceiling.breach(&exact, GIMEL).map(|breach| breach.budget),
            Some(BudgetKind::Run)
        );
        assert_eq!(Ceiling::unlimited().breach(&exact, ALEPH), None);

        // And on the task arm, which is the same boundary and a separate
        // comparison. `0.5` and `0.5` are exact in binary, so `>` here admits
        // the spawn the operator's limit has already refused and `>=` does
        // not — there is no epsilon in which the two agree.
        let task_only = Ceiling {
            run_usd: None,
            task_usd: Some(0.5),
        };
        let mut at_task = Spend::new();
        at_task.record(BET, &record(1, Some(0.5)));
        let breach = task_only
            .breach(&at_task, BET)
            .expect("reaching the task ceiling is already a refusal");
        assert_eq!(breach.budget, BudgetKind::Task);
        assert!((breach.limit_usd - 0.5).abs() < f64::EPSILON);
        assert!((breach.spent_usd - 0.5).abs() < f64::EPSILON);
        assert_eq!(
            task_only.breach(&at_task, GIMEL),
            None,
            "one task's spend was charged to another"
        );
    }

    /// The ceiling is consulted only inside an admitting branch: a run with
    /// nothing to spawn never records a refusal of a spawn.
    #[test]
    fn a_run_with_no_admissible_work_never_asks_the_ceiling() {
        let fold = all_failed();
        assert!(!fold.structurally_admissible());
        let ceiling = Ceiling {
            run_usd: Some(0.0),
            task_usd: None,
        };
        assert_eq!(
            select(&fold, &ceiling, &no_spend()),
            Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete)),
            "a breached ceiling turned an ended run into a budget stop"
        );
    }

    // -----------------------------------------------------------------------
    // checkpoint_refusals
    // -----------------------------------------------------------------------

    /// The checkpoint refusal, in the three shapes `checkpoint_refusals` and
    /// `loop` give it.
    ///
    /// A budget breach with structurally admissible work appends
    /// `budget_exceeded` **before any spawn**; integration and run end are
    /// refused **before any start append**.
    #[test]
    fn a_breach_appends_budget_exceeded_and_integration_and_run_end_are_refused() {
        // (1) A breach with work to do. `select` is a pure function — it
        // performs no effect and appends nothing — so "before any spawn" is
        // structural, and what the loop is handed is the event itself.
        let fold = started();
        assert!(
            fold.structurally_admissible() && fold.ready(ALEPH),
            "there is no spawn for the ceiling to refuse"
        );
        let ceiling = Ceiling {
            run_usd: Some(2.0),
            task_usd: None,
        };
        let mut spend = Spend::new();
        spend.record(BET, &record(1, Some(2.5)));

        let step = select(&fold, &ceiling, &spend);
        let Step::BudgetExceeded(exceeded) = step.clone() else {
            panic!("a breached ceiling admitted the dispatch: {step:?}");
        };
        assert_eq!(exceeded.epoch, Epoch(0));
        assert_eq!(exceeded.budget, BudgetKind::Run);
        assert!((exceeded.limit_usd - 2.0).abs() < f64::EPSILON);
        assert!((exceeded.spent_usd - 2.5).abs() < f64::EPSILON);
        assert_eq!(
            exceeded.key,
            Some(ALEPH),
            "the record must name the task whose next attempt was refused"
        );

        // It is not a start, so the checkpoint admits it, and the fold takes
        // it — after which the run is ending.
        assert_eq!(
            checkpoint(step).expect("a budget stop is not a start"),
            Admitted::BudgetExceeded(exceeded.clone())
        );
        let mut fold = fold;
        apply(
            &mut fold,
            &ev(TopologyEventBody::BudgetExceeded {
                data: *exceeded.clone(),
            }),
        );
        assert_eq!(
            fold.derived_outcome(),
            DerivedOutcome::Ending(RunOutcome::BudgetExceeded)
        );

        // (2) An eligible integration is refused before the
        // `merge_verification_started` that would start one.
        let mut fold = started();
        let candidate = queue_candidate(&mut fold, GIMEL, 0);
        let step = select(&fold, &Ceiling::unlimited(), &no_spend());
        assert_eq!(
            step,
            Step::Integrate {
                candidate: Box::new(candidate.clone())
            }
        );
        let error = checkpoint(step).expect_err("this build does not integrate");
        let message = format!("{error}");
        assert!(message.contains("does not integrate"), "{message}");
        assert!(
            message.contains(candidate.candidate_ref.0.as_str()),
            "the refusal does not name what it refused: {message}"
        );

        // The ceiling is checked *inside* the integration branch and before
        // it, so a breach with an eligible integration records the stop rather
        // than the refusal.
        let mut spend = Spend::new();
        spend.record(GIMEL, &record(1, Some(9.0)));
        let step = select(
            &fold,
            &Ceiling {
                run_usd: Some(1.0),
                task_usd: None,
            },
            &spend,
        );
        let Step::BudgetExceeded(exceeded) = step.clone() else {
            panic!("the ceiling was checked after the integration decision: {step:?}");
        };
        assert_eq!(exceeded.budget, BudgetKind::Run);
        assert!((exceeded.limit_usd - 1.0).abs() < f64::EPSILON);
        assert!((exceeded.spent_usd - 9.0).abs() < f64::EPSILON);
        assert_eq!(
            exceeded.key, None,
            "no task's next attempt was refused by an integration's stop"
        );
        assert!(checkpoint(step).is_ok());

        // (3) Run-end closure is refused before `run_finished`.
        let fold = all_failed();
        let step = select(&fold, &Ceiling::unlimited(), &no_spend());
        assert_eq!(
            step,
            Step::Closure(DerivedOutcome::Ending(RunOutcome::Complete))
        );
        let error = checkpoint(step).expect_err("this build does not end a run");
        assert!(format!("{error}").contains("does not end a run"), "{error}");
    }

    /// Every branch an intermediate build *is* entitled to perform survives
    /// the checkpoint unchanged.
    #[test]
    fn the_checkpoint_admits_every_branch_this_build_implements() {
        let admitted = [
            (
                Step::Retry {
                    key: BET,
                    generation: GenerationId(3),
                    attempt: AttemptNumber(4),
                },
                Admitted::Retry {
                    key: BET,
                    generation: GenerationId(3),
                    attempt: AttemptNumber(4),
                },
            ),
            (
                Step::Dispatch {
                    key: GIMEL,
                    generation: GenerationId(2),
                    continuing: false,
                },
                Admitted::Dispatch {
                    key: GIMEL,
                    generation: GenerationId(2),
                    continuing: false,
                },
            ),
            (Step::Backoff, Admitted::Backoff),
            (
                Step::HardBlock {
                    questions: vec![question_for(ALEPH).id],
                },
                Admitted::HardBlock {
                    questions: vec![question_for(ALEPH).id],
                },
            ),
        ];
        for (step, expected) in admitted {
            assert_eq!(checkpoint(step.clone()).expect("admitted"), expected);
        }
    }

    // -----------------------------------------------------------------------
    // The remaining branches
    // -----------------------------------------------------------------------

    /// The retry branch checks the ceiling before it admits the retry.
    ///
    /// Its own branch and not the dispatch branch's: `loop` puts the check
    /// inside **each** admitting branch, and `ALEPH` is `ready` here, so a
    /// selector that admitted the retry unconditionally would still have a
    /// later branch to fall through to and a `BudgetExceeded` to produce from
    /// it. The assertion is therefore on `key`: only the retry's own check
    /// names the retained task.
    #[test]
    fn the_retry_branch_checks_the_ceiling_and_names_the_retained_task() {
        let mut fold = started();
        retained_generation(&mut fold, BET, 0);
        assert!(fold.ready_retry(BET), "the retry branch is not live");
        assert!(fold.ready(ALEPH), "the branch below it is live");

        // `BET` is over its own ceiling; `ALEPH` has spent nothing.
        let ceiling = Ceiling {
            run_usd: None,
            task_usd: Some(1.5),
        };
        let mut spend = Spend::new();
        spend.record(BET, &record(1, Some(3.0)));

        let step = select(&fold, &ceiling, &spend);
        let Step::BudgetExceeded(exceeded) = step.clone() else {
            panic!("a breached ceiling admitted the retry: {step:?}");
        };
        assert_eq!(exceeded.epoch, Epoch(0));
        assert_eq!(exceeded.budget, BudgetKind::Task);
        assert!((exceeded.limit_usd - 1.5).abs() < f64::EPSILON);
        assert!((exceeded.spent_usd - 3.0).abs() < f64::EPSILON);
        assert_eq!(
            exceeded.key,
            Some(BET),
            "the stop must name the retained task whose next attempt was refused, not the \
             dispatch that would have run instead"
        );
        assert!(checkpoint(step).is_ok(), "a budget stop is not a start");

        // Under the ceiling, the same state runs the retry.
        assert_eq!(
            select(
                &fold,
                &Ceiling {
                    run_usd: None,
                    task_usd: Some(4.0),
                },
                &spend
            ),
            Step::Retry {
                key: BET,
                generation: GenerationId(0),
                attempt: AttemptNumber(2),
            }
        );
    }

    /// Backoff precedes the hard block, and the two are live at once.
    ///
    /// `loop`'s order is fixed, and no other fixture holds a `Deferred` task
    /// **and** an open question at the same time — so with the two branches
    /// swapped every one of them still passes. A deferred task is waiting on a
    /// wait that will elapse on its own; a question waits on a person. Serving
    /// the person first would park a run that was about to make progress.
    #[test]
    fn the_backoff_branch_precedes_the_hard_block_when_both_are_live() {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
        assert_eq!(fold.task_state(ALEPH), Some(TaskState::Deferred));

        in_flight(&mut fold, BET, 0);
        let mut parking = finished(BET, 0, 1, Next::AskHuman(crate::ir::QuestionKind::Unblock));
        parking.question = Some(question_for(BET));
        settle_into(&mut fold, &parking);

        in_flight(&mut fold, GIMEL, 0);
        settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));

        assert!(fold.backoff_pending(), "the backoff branch is not live");
        assert!(fold.questions_open(), "the hard-block branch is not live");
        assert!(
            !fold.structurally_admissible(),
            "a branch above both is live, and this asserts nothing about their order"
        );
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Backoff,
            "the hard-block rules were applied to a run that was about to wake"
        );

        // With the wait elapsed and the woken task run out, the question is
        // what is left — the other half of the same order.
        apply(&mut fold, &resume_event());
        assert!(!fold.backoff_pending(), "the resume woke the deferred task");
        in_flight(&mut fold, ALEPH, 1);
        settle_into(&mut fold, &finished(ALEPH, 1, 1, Next::Fail));
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::HardBlock {
                questions: vec![question_for(BET).id]
            }
        );
    }

    /// An integration is charged to the run and never to the candidate's task.
    ///
    /// `BudgetExceeded4::key` is "the task whose next attempt was refused. Not
    /// a failed task: nothing judged it and nothing was spent on it", and an
    /// integration is neither half: it is not that task's next attempt, and
    /// money *was* spent — the candidate exists because an attempt succeeded
    /// and was paid for. Charging the task ceiling would refuse the merge of
    /// work already bought, and refuse it permanently: the candidate can never
    /// integrate and the task can never unspend.
    #[test]
    fn an_integration_is_charged_to_the_run_and_never_to_the_candidates_task() {
        let mut fold = started();
        let candidate = queue_candidate(&mut fold, GIMEL, 0);

        let mut spend = Spend::new();
        spend.record(GIMEL, &record(1, Some(9.0)));
        let task_only = Ceiling {
            run_usd: None,
            task_usd: Some(1.0),
        };
        assert_eq!(
            select(&fold, &task_only, &spend),
            Step::Integrate {
                candidate: Box::new(candidate)
            },
            "a task ceiling refused the merge of work it had already paid for"
        );
        // The same ceiling still refuses that task's next *attempt*, which is
        // what it is a ceiling on.
        assert_eq!(
            task_only.breach(&spend, GIMEL).map(|breach| breach.budget),
            Some(BudgetKind::Task)
        );
    }

    /// The backoff branch, and the guard that keeps it out from under a halt
    /// or a budget stop.
    #[test]
    fn the_backoff_branch_is_entered_only_while_the_run_is_not_ending() {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        settle_into(&mut fold, &finished(ALEPH, 0, 1, Next::Defer));
        in_flight(&mut fold, BET, 0);
        settle_into(&mut fold, &finished(BET, 0, 1, Next::Fail));
        in_flight(&mut fold, GIMEL, 0);
        settle_into(&mut fold, &finished(GIMEL, 0, 1, Next::Fail));
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Backoff
        );

        // Waking it returns the task to an ordinary dispatch.
        let mut woken = fold.clone();
        apply(&mut woken, &resume_event());
        assert_eq!(
            select(&woken, &Ceiling::unlimited(), &no_spend()),
            Step::Dispatch {
                key: ALEPH,
                generation: GenerationId(1),
                continuing: false,
            }
        );

        // A halt: the branch is not offered, and the closure is.
        let mut halted = started();
        in_flight(&mut halted, ALEPH, 0);
        settle_into(&mut halted, &finished(ALEPH, 0, 1, Next::Defer));
        in_flight(&mut halted, BET, 0);
        let mut halting = finished(BET, 0, 1, Next::Fail);
        halting.halts_run = true;
        settle_into(&mut halted, &halting);
        assert_eq!(
            select(&halted, &Ceiling::unlimited(), &no_spend()),
            Step::Closure(DerivedOutcome::Ending(RunOutcome::Halted))
        );

        // A budget stop in this epoch: likewise.
        let mut stopped = started();
        in_flight(&mut stopped, ALEPH, 0);
        settle_into(&mut stopped, &finished(ALEPH, 0, 1, Next::Defer));
        apply(
            &mut stopped,
            &super::super::settle::tests::budget_exceeded(Epoch(0), BET),
        );
        assert_eq!(
            select(&stopped, &Ceiling::unlimited(), &no_spend()),
            Step::Closure(DerivedOutcome::Ending(RunOutcome::BudgetExceeded))
        );
    }

    /// **An ending run offers no work — for every arm, not for the empty fold.**
    ///
    /// `an_ending_run_reaches_closure` already asserts this, and asserts it only
    /// where **nothing else is live**. That is the scoping gap round 3
    /// harvested: the property being claimed is "an ending run proceeds to
    /// closure", and the fixture only ever tested "an idle ending run does".
    ///
    /// `PR7-R3-LOOP-001` is what got through it. `TopologyFold::open_no_attempt`
    /// is a statement accessor and — correctly, and unlike `ready`,
    /// `ready_retry` and `integration_admissible` — consults no run state, so
    /// the continuation arm offered work on a budget-stopped run. Measured end
    /// to end by that lens: five `step()` calls, five duplicate
    /// `budget_exceeded` records, no closure; and with `halted_at` set,
    /// `Dispatch { continuing: true }` — a halted run spawning a worker.
    ///
    /// So the witness is written over **every arm that can offer work**, each
    /// with its own precondition satisfied and the run ending. An arm added
    /// later fails this the moment it is reachable, which the one-arm version
    /// could not do.
    #[test]
    fn an_ending_run_offers_no_work_from_any_arm() {
        // Each case: a fold where THIS arm is live, then the same fold ended.
        // The `live` assertion is the premise — without it a case could pass
        // because its arm was never eligible in the first place.
        /// A fixture builder for one arm, named for the failure message.
        type Arm = (&'static str, fn() -> TopologyFold);

        let cases: Vec<Arm> = vec![
            ("continuation (OpenNoAttempt)", || {
                let mut fold = started();
                apply(&mut fold, &dispatch(ALEPH, 0));
                fold
            }),
            ("ready dispatch", started),
            ("ready retry (RetainedIdle)", || {
                let mut fold = started();
                retained_generation(&mut fold, BET, 0);
                fold
            }),
        ];

        for (name, build) in cases {
            let live = build();
            assert!(
                !matches!(
                    select(&live, &Ceiling::unlimited(), &no_spend()),
                    Step::Closure(_)
                ),
                "{name}: the arm is not live before the run ends, so ending it \
                 proves nothing"
            );

            let mut ending = build();
            apply(
                &mut ending,
                &super::super::settle::tests::budget_exceeded(Epoch(0), GIMEL),
            );
            assert!(
                matches!(
                    select(&ending, &Ceiling::unlimited(), &no_spend()),
                    Step::Closure(_)
                ),
                "{name}: an ending run offered work. `loop` says a breach \
                 proceeds to closure, and a run that keeps selecting an arm it \
                 then refuses appends a duplicate stop record every iteration \
                 and never terminates: {:?}",
                select(&ending, &Ceiling::unlimited(), &no_spend())
            );
        }
    }

    /// The hard-block branch: open questions and nothing else runnable.
    #[test]
    fn open_questions_reach_the_hard_block_branch_before_closure() {
        let mut fold = started();
        in_flight(&mut fold, ALEPH, 0);
        let mut parking = finished(
            ALEPH,
            0,
            1,
            Next::AskHuman(crate::ir::QuestionKind::Unblock),
        );
        parking.question = Some(question_for(ALEPH));
        settle_into(&mut fold, &parking);
        for key in [BET, GIMEL] {
            in_flight(&mut fold, key, 0);
            settle_into(&mut fold, &finished(key, 0, 1, Next::Fail));
        }
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::HardBlock {
                questions: vec![question_for(ALEPH).id]
            },
            "the loop applies the hard-block rules before it closes the run"
        );
        // Left to itself the fold would already end this run Parked, which is
        // exactly why the branch order matters.
        assert_eq!(
            fold.derived_outcome(),
            DerivedOutcome::Ending(RunOutcome::Parked)
        );
    }

    /// A poisoned fold selects nothing and is refused.
    #[test]
    fn a_poisoned_fold_selects_nothing() {
        let mut fold = started();
        assert!(fold.ready(ALEPH));
        fold.poison();
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Poisoned
        );
        let error = checkpoint(Step::Poisoned).expect_err("a poisoned fold authorises nothing");
        assert!(format!("{error}").contains("poisoned"), "{error}");
    }

    /// A fold with no `run_started` has recorded nothing, so nothing is
    /// selectable and nothing has ended.
    #[test]
    fn an_unstarted_run_selects_nothing() {
        let fold = TopologyFold::new(inputs());
        assert_eq!(
            select(&fold, &Ceiling::unlimited(), &no_spend()),
            Step::Closure(DerivedOutcome::NotEnding)
        );
        checkpoint(select(&fold, &Ceiling::unlimited(), &no_spend()))
            .expect_err("nothing is admitted from a run that has not started");
    }

    /// The retry the selector names is the one the settlement module runs.
    ///
    /// Two modules deciding "which generation, which attempt" independently is
    /// two rules that can disagree; this is the assertion that they do not.
    #[test]
    fn the_selected_retry_is_the_one_the_settlement_module_runs() {
        let mut fold = started();
        retained_generation(&mut fold, BET, 0);
        let Step::Retry {
            key,
            generation,
            attempt,
        } = select(&fold, &Ceiling::unlimited(), &no_spend())
        else {
            panic!("a retained generation is not selected for retry");
        };

        let mut reservations = super::super::identity::Reservations::new();
        let worktrees = settle::tests::FixedVerify::passing();
        let mut hooks = super::super::seams::HarnessTopologyHooks::new(std::sync::Arc::new(
            std::sync::Mutex::new(crate::topology::effects::HookHarness::new()),
        ));
        let outcome = settle::retry(
            &fold,
            &mut reservations,
            &worktrees,
            <super::super::seams::HarnessTopologyHooks as super::super::seams::TopologyHooks>::effects(&mut hooks),
            &settle::tests::retry_request(key, generation.0),
        )
        .expect("the retry runs");
        let settle::RetryOutcome::Start(started_event) = outcome else {
            panic!("a verified worktree starts the attempt");
        };
        assert_eq!(started_event.key, key);
        assert_eq!(started_event.generation, generation);
        assert_eq!(started_event.attempt, attempt);
    }
}
