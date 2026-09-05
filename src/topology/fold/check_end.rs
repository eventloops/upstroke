//! The question checks, the budget stop, and the end of a run.

use super::start::outcome_name;
use super::*;

impl RunState {
    // --- questions ---------------------------------------------------------

    pub(super) fn check_question_raised(&self, question: &FrozenQuestion) -> Result<(), FoldError> {
        const KIND: &str = "question_raised";
        self.entry(KIND, question.key)?;
        let task = self.task(KIND, question.key)?;
        // **A bare question parks a task at rest, because its answer returns
        // one.** `apply_answer` states exactly two effects for a bare question
        // — an answer returns the task to `Pending`; a decline sets it `Failed`
        // and releases its candidate and lineage holdings — and both are
        // written against the state the question parked. So the three doors
        // here admit only a task that no other event will move before the
        // answer arrives. What could move it is read off the fold's own
        // transition table, and each door names its rows.
        //
        // The state first. A bare question is raised against `Pending`,
        // `AwaitingMerge` or `Deferred`, and against nothing else:
        // * `Merged` and `Failed` are terminal (`TaskState::is_terminal`). An
        //   answer's `Pending` would un-merge or un-fail the task and a
        //   decline would re-fail it, so no answer is consistent, and a
        //   question that can never be answered parks the run for good.
        // * `AwaitingInput` already has a question open, and every path that
        //   opens one sets this state — so this door is also the
        //   one-open-question-per-task rule the rest of the fold assumes
        //   (`open_question_for` finds one; `apply_answer` returns to one). A
        //   second question would leave the first open over a task its own
        //   answer had already moved: the pass-1 review of `15c37e4` reached
        //   the wedge below that way — two questions, the first answered, the
        //   task dispatched, the second declined.
        // * `AwaitingRepair` is moved to `Merged` by its lineage's
        //   `task_merged` (`satisfies`), which asks nothing about the task's
        //   state and would orphan a question open on it.
        if !matches!(
            task.state,
            TaskState::Pending | TaskState::AwaitingMerge | TaskState::Deferred
        ) {
            return Err(FoldError::WrongTaskState {
                kind: KIND,
                key: question.key.0,
                state: task.state.name(),
                expected: "pending, awaiting merge or deferred",
            });
        }
        // Then an open generation, in any class. This was the door that was
        // missing: a bare question against a task whose generation was in
        // flight was accepted, and the decline that answered it left the task
        // `Failed` with that generation still open and its predicted lease
        // still held; `common()` then read the open generation as "not ending"
        // for the rest of the log. What the table permits per class:
        // * `InFlight` closes only by `attempt_finished` or
        //   `attempt_interrupted`, and each sets the task's state — after a
        //   decline the first fabricates a settlement for an attempt nobody
        //   judged and the second un-fails the task. `generation_closed` is
        //   refused from it. A decline here is unrecoverable.
        // * `Promoting` closes only by `task_candidate_created`, which sets
        //   `AwaitingMerge` — un-failing the task. Unrecoverable likewise.
        // * `OpenNoAttempt` and `RetainedIdle` **are** closable after a
        //   decline: `generation_closed` accepts both classes whatever the
        //   task's state, and the engine closes an `OpenNoAttempt` generation
        //   at run end with `RunEnding` (`dispatch.rs::close_at_run_end`). But
        //   `attempt_started` asks nothing about the task's state either, so an
        //   attempt can start — or resume — in that generation while the
        //   question is open, and the decline then lands on `InFlight`. The
        //   fold does not order the human's answer against the attempt's
        //   start, so the door covers these two classes for the same reason as
        //   the first two, and not because they cannot close.
        // Every other path that opens a question already has no open
        // generation to leave behind: a parking settlement closes its
        // generation on the same event, an admission question is asked at
        // registration, and a verification park is asked of a candidate whose
        // generation closed when the candidate was created.
        if let Some(open) = task.open() {
            return Err(FoldError::GenerationOpen {
                kind: KIND,
                key: question.key.0,
                generation: open.id.0,
                class: open.class.name(),
            });
        }
        // Then the open integration transaction. Its terminal events move the
        // task without asking its state — `task_merged` to `Merged`,
        // `merge_rejected` to `AwaitingRepair` — so a question open on the
        // candidate's task would be orphaned by either, and an answer after
        // `task_merged` would un-merge the task. `first_eligible` already
        // skips a parked task's candidate when a transaction *starts*; this is
        // the same rule for one that is already open. (`ready` and
        // `ready_retry` carry the same clause.)
        if let Some(open) = self
            .transaction
            .as_ref()
            .filter(|open| open.candidate.key == question.key)
        {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it parks task {} while that task's candidate (generation {}) is the open \
                     integration transaction, sequence {}; a parked task's candidate is not \
                     integrated, and the transaction's terminal event would move the task under \
                     its question",
                    question.key.0, open.candidate.generation.0, open.sequence.0
                ),
            });
        }
        self.check_new_question(KIND, question, question.key)
    }

    pub(super) fn check_question_answered(
        &self,
        answered: &QuestionAnswered4,
    ) -> Result<QuestionOrigin, FoldError> {
        const KIND: &str = "question_answered";
        // refusals[20]: answers are not ingested in an epoch after a halting
        // settlement or a budget stop.
        if self.halted_epoch == Some(self.epoch) {
            return Err(FoldError::RunEnding {
                kind: KIND,
                what: "a halting settlement",
            });
        }
        if self.budget_stop_is_current() {
            return Err(FoldError::RunEnding {
                kind: KIND,
                what: "the budget stop",
            });
        }
        // refusals[13], A1's half: the answer must agree with itself.
        answered
            .self_consistency()
            .map_err(|defect| FoldError::InconsistentRecord {
                kind: KIND,
                detail: defect.to_string(),
            })?;

        let open =
            self.questions
                .get(&answered.question)
                .ok_or_else(|| FoldError::WrongQuestion {
                    kind: KIND,
                    question: answered.question.to_string(),
                    detail: if self.seen_questions.contains(&answered.question) {
                        "has already been answered; a question is answered once".to_owned()
                    } else {
                        "this log never asked".to_owned()
                    },
                })?;
        if open.question.key != answered.key {
            return Err(FoldError::WrongQuestion {
                kind: KIND,
                question: answered.question.to_string(),
                detail: format!(
                    "was asked about task {} and this answers it for task {}",
                    open.question.key, answered.key
                ),
            });
        }
        // **The answer applies to the task as the question parked it.** Every
        // path that opens a question sets `AwaitingInput` and leaves no
        // generation open, and `apply_answer`'s two effects are written
        // against exactly that. `check_question_raised` keeps a bare question
        // from being asked of a task that something else will move, and this
        // is the same rule at the other end: whatever the log did between the
        // question and its answer, an answer is applied only to a task that is
        // still parked with nothing open. Without it the raise-time doors are
        // a claim about one ordering — the pass-1 review of `15c37e4` reached
        // the wedge by answering one question, dispatching, and declining
        // another.
        let task = self.task(KIND, answered.key)?;
        if task.state != TaskState::AwaitingInput {
            return Err(FoldError::WrongTaskState {
                kind: KIND,
                key: answered.key.0,
                state: task.state.name(),
                expected: "awaiting input",
            });
        }
        if let Some(generation) = task.open() {
            return Err(FoldError::GenerationOpen {
                kind: KIND,
                key: answered.key.0,
                generation: generation.id.0,
                class: generation.class.name(),
            });
        }
        if let Answer4::Answered {
            option_index,
            binding_override,
        } = &answered.answer
        {
            let options = open.question.options.len();
            let chosen = usize::try_from(*option_index).unwrap_or(usize::MAX);
            if chosen >= options {
                return Err(FoldError::WrongQuestion {
                    kind: KIND,
                    question: answered.question.to_string(),
                    detail: format!("offered {options} option(s) and this chose {option_index}"),
                });
            }
            // refusals[12] / `task_registry.binding_override`: an override is
            // validated "against the frozen options of that task's open
            // HumanBinding question". A1's `self_consistency` has already
            // proved the override names this answer's task, question and
            // option; what is left — and what no other check makes — is that
            // there *is* such an authority and that the agent it names is the
            // one that authority froze at that index.
            match (binding_override, &open.binding) {
                (Some(_), None) => {
                    return Err(FoldError::WrongQuestion {
                        kind: KIND,
                        question: answered.question.to_string(),
                        detail: "carries a binding override and did not ask for a binding; only a \
                                 HumanBinding admission authorizes one"
                            .to_owned(),
                    });
                }
                (None, Some(_)) => {
                    return Err(FoldError::WrongQuestion {
                        kind: KIND,
                        question: answered.question.to_string(),
                        detail: "asked for a binding and this answer names none, so its task has \
                                 no binding to run"
                            .to_owned(),
                    });
                }
                (Some(binding), Some(authorized)) => {
                    let Some(agent) = authorized.get(chosen) else {
                        return Err(FoldError::WrongQuestion {
                            kind: KIND,
                            question: answered.question.to_string(),
                            detail: format!(
                                "authorized {} binding(s) and this chose {option_index}",
                                authorized.len()
                            ),
                        });
                    };
                    if binding.agent != *agent {
                        return Err(FoldError::WrongQuestion {
                            kind: KIND,
                            question: answered.question.to_string(),
                            detail: format!(
                                "authorized `{agent}` at option {option_index} and the override \
                                 names `{}`",
                                binding.agent
                            ),
                        });
                    }
                }
                (None, None) => {}
            }
        }
        Ok(open.origin)
    }

    // --- budget_exceeded ---------------------------------------------------

    pub(super) fn check_budget_exceeded(
        &self,
        exceeded: &BudgetExceeded4,
    ) -> Result<(), FoldError> {
        const KIND: &str = "budget_exceeded";
        if let Some(key) = exceeded.key {
            self.entry(KIND, key)?;
        }
        if exceeded.epoch != self.epoch {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it belongs to epoch {} and this run is in epoch {}",
                    exceeded.epoch.0, self.epoch.0
                ),
            });
        }
        Ok(())
    }

    // --- run_finished ------------------------------------------------------

    pub(super) fn check_run_finished(&self, finished: &RunFinished4) -> Result<(), FoldError> {
        // refusals[19] / INV-15: the recorded outcome is the derived one, and
        // the derived one is not NotEnding.
        let derived = self.derived_outcome();
        let matches = match &derived {
            DerivedOutcome::Ending(outcome) => *outcome == finished.outcome,
            DerivedOutcome::NotEnding | DerivedOutcome::FoldError => false,
        };
        if !matches {
            return Err(FoldError::OutcomeMismatch {
                recorded: outcome_name(&finished.outcome),
                derived: match &derived {
                    DerivedOutcome::NotEnding => "not ending".to_owned(),
                    DerivedOutcome::Ending(outcome) => outcome_name(outcome).to_owned(),
                    DerivedOutcome::FoldError => "unreachable".to_owned(),
                },
            });
        }
        if finished.halted_at != self.halted_at {
            return Err(FoldError::InconsistentRecord {
                kind: "run_finished",
                detail: format!(
                    "it attributes the halt to {:?} and the fold recorded {:?}",
                    finished.halted_at.map(|key| key.0),
                    self.halted_at.map(|key| key.0)
                ),
            });
        }
        Ok(())
    }
}
