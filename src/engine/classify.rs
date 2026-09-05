//! Extended notes: `docs/internals/engine/classify.md`

use crate::events::{AttemptRecord, FailureRecord, ReviewPassOutcome, ReviewRecord};
use crate::gates::{self, GateFailure};
use crate::ir::TaskKind;
use crate::ladder::{AttemptFailure, FailureKind};
use crate::review;

#[must_use]
pub(crate) fn diff_failure(
    diff: &str,
    kind: TaskKind,
    has_reviewers: bool,
) -> Option<AttemptFailure> {
    if let Some(error) = review::complete_diff_error(diff) {
        if matches!(error, review::CompleteDiffError::Opaque) || has_reviewers {
            let failure_kind = match error {
                review::CompleteDiffError::Opaque => FailureKind::ReviewInputOpaque,
                review::CompleteDiffError::TooLarge { .. } => FailureKind::ReviewInputTooLarge,
            };
            return Some(AttemptFailure::new(failure_kind, error.to_string()).from_reviewer());
        }
    }
    if kind == TaskKind::Test && !gates::diff_adds_tests(diff) {
        return Some(
            AttemptFailure::new(
                FailureKind::TestProvenance,
                "test provenance: this Test task adds no test code — a Test task that changes no \
                 tests proves nothing",
            )
            .with_feedback(
                "The diff contains no test code. Add tests that would fail without your change."
                    .to_owned(),
            ),
        );
    }
    None
}

#[must_use]
pub(crate) fn gate_failure(failure: &GateFailure) -> AttemptFailure {
    AttemptFailure::new(
        FailureKind::GateFailed,
        format!("gate `{}` failed: {}", failure.gate, failure.summary),
    )
    .with_feedback(failure.log_tail.clone())
}

pub(crate) struct ReviewPassFacts<'a> {
    pub(crate) pass: &'a str,
    pub(crate) agent: &'a str,
    pub(crate) model: &'a str,
    pub(crate) adapter: &'a str,
    pub(crate) preflight_cli_version: Option<String>,
    pub(crate) effort: Option<crate::ir::Effort>,
    pub(crate) pool: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) unavailable: bool,
    pub(crate) failed: bool,
}

impl ReviewPassFacts<'_> {
    #[must_use]
    pub(crate) fn record(self) -> ReviewRecord {
        ReviewRecord {
            pass: self.pass.to_owned(),
            agent: self.agent.to_owned(),
            model: self.model.to_owned(),
            adapter: Some(self.adapter.to_owned()),
            preflight_cli_version: self.preflight_cli_version,
            effort: self.effort,
            pool: self.pool,
            cost_usd: self.cost_usd,
            outcome: match (self.unavailable, self.failed) {
                (true, _) => ReviewPassOutcome::Unavailable,
                (false, false) => ReviewPassOutcome::Passed,
                (false, true) => ReviewPassOutcome::Failed,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeedbackCarrier {
    LadderEvent,
    AttemptRecord,
}

pub(crate) struct AttemptFacts<'a> {
    pub(crate) tier: crate::ir::Tier,
    pub(crate) model: &'a str,
    pub(crate) pool: Option<String>,
    pub(crate) resumed: bool,
    pub(crate) outcome: &'a crate::ir::Outcome,
    pub(crate) reviews: &'a [crate::events::ReviewRecord],
    pub(crate) failure: Option<&'a AttemptFailure>,
    pub(crate) feedback: FeedbackCarrier,
}

pub(crate) fn attempt_record(attempt: u32, facts: AttemptFacts<'_>) -> AttemptRecord {
    AttemptRecord {
        attempt,
        tier: facts.tier.to_string(),
        model: facts.model.to_owned(),
        pool: facts.pool,
        resumed: facts.resumed,
        duration: facts.outcome.duration,
        cost_usd: facts.outcome.cost_usd,
        reviews: facts.reviews.to_vec(),
        session_id: facts.outcome.session_id.clone(),
        usage: facts.outcome.usage.clone(),
        failure: facts.failure.map(|failure| FailureRecord {
            kind: failure.kind,
            origin: failure.origin,
            reason: failure.reason.clone(),
            detail: match facts.feedback {
                FeedbackCarrier::AttemptRecord => failure.feedback.clone(),
                FeedbackCarrier::LadderEvent => None,
            },
        }),
    }
}

pub(crate) fn review_input_failure(problem: String) -> AttemptFailure {
    AttemptFailure::new(FailureKind::ReviewInputOpaque, problem).from_reviewer()
}
