//! Extended notes: `docs/internals/ladder.md`

use crate::ir::QuestionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    NoChain,
    EmptyDiff,
    AgentError,
    Timeout,
    RateLimited,
    GateFailed,
    TestProvenance,
    ReviewInputTooLarge,
    ReviewInputOpaque,
    ReviewFailed,
    ReviewUnavailable,
    NeedsHuman,
    Declined,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureOrigin {
    Worker,
    Reviewer,
}

#[derive(Debug, Clone)]
pub struct AttemptFailure {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
    pub reason: String,
    pub feedback: Option<String>,
}

impl AttemptFailure {
    pub fn new(kind: FailureKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            origin: FailureOrigin::Worker,
            reason: reason.into(),
            feedback: None,
        }
    }

    pub fn with_feedback(mut self, feedback: String) -> Self {
        self.feedback = Some(feedback);
        self
    }

    pub fn from_reviewer(mut self) -> Self {
        self.origin = FailureOrigin::Reviewer;
        self
    }

    pub fn is_outage(&self) -> bool {
        FailureShape::of(self).is_outage()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LadderPolicy {
    pub attempts_per: u32,
    pub rungs: usize,
    pub max_defers: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct LadderState {
    pub rung: usize,
    pub attempts_on_rung: u32,
    pub defers: u32,
    pub resumable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    RetrySameRung { resume: bool },
    Escalate,
    Defer,
    AskHuman(QuestionKind),
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureShape {
    pub kind: FailureKind,
    pub origin: FailureOrigin,
}

impl FailureShape {
    #[must_use]
    pub const fn of(failure: &AttemptFailure) -> Self {
        Self {
            kind: failure.kind,
            origin: failure.origin,
        }
    }

    #[must_use]
    pub fn is_outage(self) -> bool {
        matches!(
            (self.kind, self.origin),
            (FailureKind::RateLimited | FailureKind::ReviewUnavailable, _)
                | (FailureKind::Timeout, FailureOrigin::Reviewer)
        )
    }
}

#[must_use]
pub fn spends_allowance(failure: Option<FailureShape>) -> bool {
    let Some(failure) = failure else {
        return true;
    };

    if failure.is_outage() {
        return false;
    }

    match failure.kind {
        FailureKind::NeedsHuman => false,
        FailureKind::NoChain => false,
        FailureKind::Interrupted => false,
        FailureKind::Declined => false,
        FailureKind::EmptyDiff
        | FailureKind::AgentError
        | FailureKind::Timeout
        | FailureKind::RateLimited
        | FailureKind::GateFailed
        | FailureKind::TestProvenance
        | FailureKind::ReviewInputTooLarge
        | FailureKind::ReviewInputOpaque
        | FailureKind::ReviewFailed
        | FailureKind::ReviewUnavailable => true,
    }
}

pub fn next_step(failure: &AttemptFailure, state: &LadderState, policy: &LadderPolicy) -> Next {
    if failure.kind == FailureKind::NeedsHuman {
        return Next::AskHuman(QuestionKind::Clarify);
    }

    if matches!(
        failure.kind,
        FailureKind::ReviewInputTooLarge | FailureKind::ReviewInputOpaque
    ) {
        return Next::AskHuman(QuestionKind::Unblock);
    }

    if failure.is_outage() {
        return if state.defers < policy.max_defers {
            Next::Defer
        } else {
            Next::AskHuman(QuestionKind::Unblock)
        };
    }

    if failure.kind == FailureKind::NoChain || policy.rungs == 0 {
        return Next::Fail;
    }

    if state.attempts_on_rung < policy.attempts_per {
        return Next::RetrySameRung {
            resume: state.resumable,
        };
    }
    if state.rung + 1 < policy.rungs {
        return Next::Escalate;
    }
    Next::AskHuman(QuestionKind::Unblock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spends_allowance_matches_every_legacy_park_path() {
        let grid: Vec<(FailureKind, FailureOrigin, bool, &str)> = vec![
            (
                FailureKind::NeedsHuman,
                FailureOrigin::Reviewer,
                false,
                "Asked for a human explicitly: the code was never judged, so \
                 nothing is spent and nothing escalates.",
            ),
            (
                FailureKind::ReviewInputTooLarge,
                FailureOrigin::Reviewer,
                true,
                "The worker ran, so the attempt is spent and must stay in the \
                 ledger, but no amount of automatic retrying can make the same \
                 complete diff fit the review contract.",
            ),
            (
                FailureKind::ReviewInputOpaque,
                FailureOrigin::Reviewer,
                true,
                "The worker ran, so the attempt is spent and must stay in the \
                 ledger.",
            ),
            (
                FailureKind::RateLimited,
                FailureOrigin::Reviewer,
                false,
                "Outages defer. Escalating here would move the task to a \
                 pricier rung because a *pool* was busy, and retrying would \
                 burn attempts on a run that never got a verdict.",
            ),
        ];

        for (kind, origin, spends, why) in grid {
            let mut failure = AttemptFailure::new(kind, "fixture");
            if origin == FailureOrigin::Reviewer {
                failure = failure.from_reviewer();
            }
            assert_eq!(
                spends_allowance(Some(FailureShape::of(&failure))),
                spends,
                "{kind:?} must {} the rung's allowance — {why}",
                if spends { "spend" } else { "not spend" }
            );
        }
    }

    #[test]
    fn chain_exhaustion_parks_only_after_the_allowance_was_already_spent() {
        let policy = LadderPolicy {
            rungs: 1,
            attempts_per: 2,
            max_defers: 1,
        };
        let rejection = AttemptFailure::new(FailureKind::ReviewFailed, "rejected");
        assert!(
            spends_allowance(Some(FailureShape::of(&rejection))),
            "a real rejection spends, which is what walks the counter up"
        );

        let fresh = LadderState {
            rung: 0,
            attempts_on_rung: 0,
            defers: 0,
            resumable: false,
        };
        assert!(
            matches!(
                next_step(&rejection, &fresh, &policy),
                Next::RetrySameRung { .. }
            ),
            "below the allowance it retries the rung"
        );

        let spent = LadderState {
            attempts_on_rung: policy.attempts_per,
            ..fresh
        };
        assert!(
            matches!(next_step(&rejection, &spent, &policy), Next::AskHuman(_)),
            "and parks only once the allowance is gone — so this park follows \
             the spending rather than causing it"
        );
    }

    fn policy() -> LadderPolicy {
        LadderPolicy {
            attempts_per: 2,
            rungs: 3,
            max_defers: 3,
        }
    }

    fn state(rung: usize, attempts_on_rung: u32) -> LadderState {
        LadderState {
            rung,
            attempts_on_rung,
            defers: 0,
            resumable: true,
        }
    }

    fn failure(kind: FailureKind) -> AttemptFailure {
        AttemptFailure::new(kind, "because")
    }

    fn every_failure_kind() -> Vec<FailureKind> {
        const HEADER: &str = "pub enum FailureKind {";
        let source = include_str!("ladder.rs");
        let body = &source[source.find(HEADER).expect("the enum is declared") + HEADER.len()..];
        let body = &body[..body.find("\n}").expect("the enum closes")];
        let names: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.ends_with(',')
                    && line[..line.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric())
                    && line.starts_with(|c: char| c.is_ascii_uppercase())
            })
            .map(|line| &line[..line.len() - 1])
            .collect();
        assert!(
            names.len() >= 10,
            "the source read found {} variants, which is too few to be this enum — the \
             parse is broken, not the enum",
            names.len()
        );
        names.into_iter().map(kind_of_name).collect()
    }

    fn kind_of_name(name: &str) -> FailureKind {
        let named = |kind: FailureKind| -> &'static str {
            match kind {
                FailureKind::NoChain => "NoChain",
                FailureKind::EmptyDiff => "EmptyDiff",
                FailureKind::AgentError => "AgentError",
                FailureKind::Timeout => "Timeout",
                FailureKind::RateLimited => "RateLimited",
                FailureKind::GateFailed => "GateFailed",
                FailureKind::TestProvenance => "TestProvenance",
                FailureKind::ReviewInputTooLarge => "ReviewInputTooLarge",
                FailureKind::ReviewInputOpaque => "ReviewInputOpaque",
                FailureKind::ReviewFailed => "ReviewFailed",
                FailureKind::ReviewUnavailable => "ReviewUnavailable",
                FailureKind::NeedsHuman => "NeedsHuman",
                FailureKind::Declined => "Declined",
                FailureKind::Interrupted => "Interrupted",
            }
        };
        for kind in [
            FailureKind::NoChain,
            FailureKind::EmptyDiff,
            FailureKind::AgentError,
            FailureKind::Timeout,
            FailureKind::RateLimited,
            FailureKind::GateFailed,
            FailureKind::TestProvenance,
            FailureKind::ReviewInputTooLarge,
            FailureKind::ReviewInputOpaque,
            FailureKind::ReviewFailed,
            FailureKind::ReviewUnavailable,
            FailureKind::NeedsHuman,
            FailureKind::Declined,
            FailureKind::Interrupted,
        ] {
            if named(kind) == name {
                return kind;
            }
        }
        panic!(
            "`{name}` is a variant of `FailureKind` that this mapping does not name; the \
             exhaustive match above compiles, so the candidate list beneath it is what is \
             short"
        )
    }

    #[test]
    fn exactly_thirteen_failure_shapes_spend_no_allowance() {
        let mut free: Vec<(FailureKind, FailureOrigin)> = Vec::new();
        for kind in every_failure_kind() {
            for origin in [FailureOrigin::Worker, FailureOrigin::Reviewer] {
                if !spends_allowance(Some(FailureShape { kind, origin })) {
                    free.push((kind, origin));
                }
            }
        }

        assert_eq!(
            free.len(),
            13,
            "{} `(kind, origin)` shapes spend nothing, not 13: {free:?}. \
             `Settled::spent_attempt` quotes this number",
            free.len()
        );

        let kinds: std::collections::BTreeSet<String> =
            free.iter().map(|(kind, _)| format!("{kind:?}")).collect();
        assert_eq!(
            kinds.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "Declined",
                "Interrupted",
                "NeedsHuman",
                "NoChain",
                "RateLimited",
                "ReviewUnavailable",
                "Timeout",
            ],
            "the seven kinds those shapes span changed, so the doc naming them is wrong"
        );

        assert_eq!(
            free.iter()
                .filter(|(kind, _)| *kind == FailureKind::Timeout)
                .count(),
            1,
            "`Timeout` is the one kind whose answer depends on the origin"
        );
        assert!(
            spends_allowance(Some(FailureShape {
                kind: FailureKind::Timeout,
                origin: FailureOrigin::Worker,
            })),
            "a worker that ran out of wall clock still ran"
        );
        assert!(
            !spends_allowance(Some(FailureShape {
                kind: FailureKind::Timeout,
                origin: FailureOrigin::Reviewer,
            })),
            "a reviewer that never answered judged nothing"
        );
    }

    #[test]
    fn a_rejected_attempt_retries_the_same_rung_until_attempts_per() {
        for kind in [
            FailureKind::EmptyDiff,
            FailureKind::AgentError,
            FailureKind::Timeout,
            FailureKind::GateFailed,
            FailureKind::TestProvenance,
            FailureKind::ReviewFailed,
        ] {
            assert_eq!(
                next_step(&failure(kind), &state(0, 1), &policy()),
                Next::RetrySameRung { resume: true },
                "{kind:?} should retry with feedback"
            );
        }
    }

    #[test]
    fn resume_follows_the_adapter_not_the_failure() {
        let mut cold = state(0, 1);
        cold.resumable = false;
        assert_eq!(
            next_step(&failure(FailureKind::GateFailed), &cold, &policy()),
            Next::RetrySameRung { resume: false }
        );
    }

    #[test]
    fn exhausting_a_rung_escalates_and_exhausting_the_chain_asks_a_human() {
        let policy = policy();
        assert_eq!(
            next_step(&failure(FailureKind::GateFailed), &state(0, 2), &policy),
            Next::Escalate
        );
        assert_eq!(
            next_step(&failure(FailureKind::GateFailed), &state(1, 2), &policy),
            Next::Escalate
        );
        assert_eq!(
            next_step(&failure(FailureKind::GateFailed), &state(2, 2), &policy),
            Next::AskHuman(QuestionKind::Unblock),
            "the human is the top rung (§11.4)"
        );
    }

    #[test]
    fn an_oversized_complete_review_parks_without_paying_for_a_retry() {
        for kind in [
            FailureKind::ReviewInputTooLarge,
            FailureKind::ReviewInputOpaque,
        ] {
            assert_eq!(
                next_step(&failure(kind), &state(0, 1), &policy()),
                Next::AskHuman(QuestionKind::Unblock)
            );
        }
    }

    #[test]
    fn a_single_rung_chain_goes_straight_from_attempts_to_the_human() {
        let policy = LadderPolicy {
            attempts_per: 1,
            rungs: 1,
            max_defers: 3,
        };
        assert_eq!(
            next_step(&failure(FailureKind::ReviewFailed), &state(0, 1), &policy),
            Next::AskHuman(QuestionKind::Unblock)
        );
    }

    #[test]
    fn rate_limits_defer_without_spending_an_attempt() {
        let mut last = state(2, 2);
        assert_eq!(
            next_step(&failure(FailureKind::RateLimited), &last, &policy()),
            Next::Defer
        );
        last.defers = 3;
        assert_eq!(
            next_step(&failure(FailureKind::RateLimited), &last, &policy()),
            Next::AskHuman(QuestionKind::Unblock),
            "a pool that never came back is a real blocker"
        );
    }

    #[test]
    fn an_unavailable_reviewer_is_an_outage_not_a_rejection() {
        for kind in [FailureKind::ReviewUnavailable, FailureKind::RateLimited] {
            let f = failure(kind).from_reviewer();
            assert!(f.is_outage());
            assert_eq!(next_step(&f, &state(0, 1), &policy()), Next::Defer);
        }
        let timed_out = failure(FailureKind::Timeout).from_reviewer();
        assert!(timed_out.is_outage());
        assert_eq!(next_step(&timed_out, &state(0, 1), &policy()), Next::Defer);
    }

    #[test]
    fn an_implementer_timeout_is_a_rejection_even_though_a_reviewer_timeout_is_not() {
        let worker = failure(FailureKind::Timeout);
        assert!(!worker.is_outage());
        assert_eq!(
            next_step(&worker, &state(0, 2), &policy()),
            Next::Escalate,
            "§19: agent timeout is an attempt failure"
        );
        assert_eq!(
            next_step(&worker.clone().from_reviewer(), &state(0, 2), &policy()),
            Next::Defer,
            "the same timeout on the judge must not escalate the implementer"
        );
    }

    #[test]
    fn needs_human_parks_immediately_from_either_side() {
        for f in [
            failure(FailureKind::NeedsHuman),
            failure(FailureKind::NeedsHuman).from_reviewer(),
        ] {
            assert_eq!(
                next_step(&f, &state(0, 1), &policy()),
                Next::AskHuman(QuestionKind::Clarify)
            );
            assert_eq!(
                next_step(&f, &state(2, 2), &policy()),
                Next::AskHuman(QuestionKind::Clarify)
            );
        }
    }

    #[test]
    fn an_empty_chain_fails_rather_than_looping() {
        assert_eq!(
            next_step(&failure(FailureKind::NoChain), &state(0, 1), &policy()),
            Next::Fail
        );
        let empty = LadderPolicy {
            attempts_per: 2,
            rungs: 0,
            max_defers: 3,
        };
        assert_eq!(
            next_step(&failure(FailureKind::GateFailed), &state(0, 1), &empty),
            Next::Fail,
            "no rung to retry on"
        );
    }

    #[test]
    fn feedback_survives_construction() {
        let f = AttemptFailure::new(FailureKind::GateFailed, "gate `test` failed")
            .with_feedback("error[E0308]: mismatched types".to_owned());
        assert_eq!(f.origin, FailureOrigin::Worker);
        assert_eq!(
            f.feedback.as_deref(),
            Some("error[E0308]: mismatched types")
        );
        assert_eq!(f.from_reviewer().origin, FailureOrigin::Reviewer);
    }
}
