//! Extended notes: `docs/internals/topology/fold/check_end.md`

use super::start::outcome_name;
use super::*;

impl RunState {
    pub(super) fn check_question_raised(&self, question: &FrozenQuestion) -> Result<(), FoldError> {
        const KIND: &str = "question_raised";
        self.entry(KIND, question.key)?;
        let task = self.task(KIND, question.key)?;
        match task.state {
            TaskState::Merged | TaskState::Failed => {
                return Err(FoldError::WrongTaskState {
                    kind: KIND,
                    key: question.key.0,
                    state: task.state.name(),
                    expected: "nonterminal",
                });
            }
            TaskState::Pending
            | TaskState::Deferred
            | TaskState::AwaitingInput
            | TaskState::AwaitingMerge
            | TaskState::AwaitingRepair => {}
        }
        self.check_question_can_park_lineage(KIND, question.key)?;
        self.check_new_question(KIND, question, question.key)
    }

    pub(super) fn check_question_can_park_lineage(
        &self,
        kind: &'static str,
        key: TaskKey,
    ) -> Result<(), FoldError> {
        let root = self.lineage_root(key);
        if let Some(transaction) = &self.transaction {
            if self.lineage_root(transaction.candidate.key) == root {
                return Err(FoldError::InconsistentRecord {
                    kind,
                    detail: format!(
                        "lineage {root} has unresolved integration sequence {}; settle it before \
                         parking its tasks",
                        transaction.sequence.0
                    ),
                });
            }
        }
        for entry in self.registry.entries() {
            if self.lineage_root(entry.key) != root {
                continue;
            }
            if let Some(generation) = self.tasks.get(entry.key.index()).and_then(TaskFold::open) {
                return Err(FoldError::InconsistentRecord {
                    kind,
                    detail: format!(
                        "lineage {root} has task {} generation {} still {}; settle it before \
                         parking its tasks",
                        entry.key,
                        generation.id.0,
                        generation.class.name()
                    ),
                });
            }
        }
        Ok(())
    }

    pub(super) fn check_question_answered(
        &self,
        answered: &QuestionAnswered4,
    ) -> Result<QuestionOrigin, FoldError> {
        const KIND: &str = "question_answered";
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
        match &answered.answer {
            Answer4::Answered { .. } => {}
            Answer4::Declined { .. } => {
                if let Some(transaction) = &self.transaction {
                    if self.lineage_root(transaction.candidate.key)
                        == self.lineage_root(answered.key)
                    {
                        match &transaction.class {
                            TransactionClass::VerificationStarted { .. } => {}
                            TransactionClass::Prepared { .. } => {
                                return Err(FoldError::InconsistentRecord {
                                    kind: KIND,
                                    detail: format!(
                                        "integration sequence {} already authorizes publication; \
                                         complete it before declining its lineage",
                                        transaction.sequence.0
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Answer4::Answered {
            option_index,
            binding_override,
        } = &answered.answer
        {
            let options = open.question.options.len();
            let Some(chosen) = usize::try_from(*option_index)
                .ok()
                .filter(|chosen| *chosen < options)
            else {
                return Err(FoldError::WrongQuestion {
                    kind: KIND,
                    question: answered.question.to_string(),
                    detail: format!("offered {options} option(s) and this chose {option_index}"),
                });
            };
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

    pub(super) fn check_budget_exceeded(
        &self,
        exceeded: &BudgetExceeded4,
    ) -> Result<(), FoldError> {
        const KIND: &str = "budget_exceeded";
        if !exceeded.limit_usd.is_finite() || !exceeded.spent_usd.is_finite() {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it records a `{}` ceiling of {} and a spend of {}, and a budget is a finite \
                     number of dollars",
                    exceeded.budget, exceeded.limit_usd, exceeded.spent_usd
                ),
            });
        }
        if exceeded.spent_usd < exceeded.limit_usd {
            return Err(FoldError::InconsistentRecord {
                kind: KIND,
                detail: format!(
                    "it stops the run for a `{}` ceiling of {} that its own recorded spend of {} \
                     has not reached",
                    exceeded.budget, exceeded.limit_usd, exceeded.spent_usd
                ),
            });
        }
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

    pub(super) fn check_run_finished(&self, finished: &RunFinished4) -> Result<(), FoldError> {
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
