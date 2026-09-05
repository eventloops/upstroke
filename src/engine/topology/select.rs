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
//! integration through the fold's question-aware reader; ascending task key
//! answers it for a dispatch and a retry,
//! which is §14's "lowest plan index first" over the dense registry keys.

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::events::{AttemptRecord, BudgetKind};
use crate::ir::QuestionId;
use crate::topology::events::{
    AttemptNumber, BudgetExceeded4, CandidateRef, DerivedOutcome, Epoch, GenerationId,
    TopologyEvent, TopologyEventBody,
};
use crate::topology::fold::{GenerationClass, TopologyFold};
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
        // **One attempt, one contribution — and now by construction rather than
        // by filtering.** This kept a `BTreeSet` of attempt identities because
        // a successful attempt's record was appended *twice*: once on
        // `attempt_finished{Succeeded}` and once on `candidate_prepared`.
        // Counting each occurrence priced every successful attempt twice, and
        // only on replay, so a live total and a replay of that run's own log
        // disagreed — the deduplication existed to hide that.
        //
        // The `bf927f3` review named the dedup as evidence of the duplicate
        // rather than a licence for it, and the 2026-08-27 ruling agreed:
        // `candidate_prepared` is the sole successful settlement, the fold
        // refuses either half of the old pair, and an attempt's record now
        // reaches the log exactly once. A failure arrives on `attempt_finished`
        // and a success on `candidate_prepared`, and no attempt produces both.
        //
        // Removing it is the point. A filter that survives the shape it was
        // written for would keep a *second* reading of "one settlement per
        // attempt" alive beside the fold's, free to disagree with it — and the
        // one place that rule is enforced should be the one place it is stated.
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
/// [`Step`] has **eight** variants and this has five, so **three** do not
/// cross: `Integrate`, `Closure` and `Poisoned`. The first two are the whole of
/// `checkpoint_refusals` for PR7 — there is no value of this type that can
/// carry an integration or a run end, so no caller holding one can append
/// `merge_verification_started` or `run_finished`. That is the refusal made
/// unrepresentable rather than remembered.
///
/// The third is not a refusal of a *branch*. `Poisoned` is the absence of one:
/// an append errored, this process's fold is not authoritative, and nothing
/// further is selected at all. It is excluded from this type for the same
/// reason the other two are — a caller holding an `Admitted` may act — but not
/// for the same cause, and the count said "seven" and "two" until 2026-08-27
/// precisely by folding it into them.
///
/// Both counts are computed, per §22:
///
/// ```text
/// $ awk '/^pub enum Step \{/,/^\}/'     src/engine/topology/select.rs | grep -cE '^    [A-Z]'
/// 8
/// $ awk '/^pub enum Admitted \{/,/^\}/' src/engine/topology/select.rs | grep -cE '^    [A-Z]'
/// 5
/// ```
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
/// The fold supplies both eligibility and identity, including questions on
/// other members of the candidate's lineage. The selected step owns its
/// candidate snapshot after this borrowed view ends.
fn eligible_integration(fold: &TopologyFold) -> Option<CandidateRef> {
    fold.eligible_integration_candidate().cloned()
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
mod tests;
