//! The verification ladder's decision (DESIGN.md §11.4, §19).
//!
//! One pure function answers the only question the engine has after an attempt
//! fails: *what now?* Retry the same rung with feedback, escalate to the next
//! rung on a fresh session, defer without spending an attempt, park the task
//! behind a question, or fail it.
//!
//! It is deliberately I/O-free and holds no state of its own. The two rules
//! that are easy to get wrong live here, in one place, where they can be
//! tested exhaustively:
//!
//! 1. **Not every failure is the worker's fault.** A rate-limited pool or an
//!    unavailable reviewer is an outage (§19). Those defer; they must never
//!    burn an attempt, escalate the task to a more expensive rung, or count
//!    toward exhausting the chain — that would spend frontier tokens to
//!    "fix" code nobody found a problem with.
//! 2. **The human is the top rung** (§11.4). Running out of chain is not a
//!    failure; it is an `Unblock` question.

use crate::ir::QuestionKind;

/// Why an attempt did not pass. Kept apart from the prose reason so the ladder
/// can dispatch on it and the event log (step 8) can aggregate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The resolved chain has no rungs — a config defect, not a task failure.
    NoChain,
    EmptyDiff,
    AgentError,
    Timeout,
    RateLimited,
    GateFailed,
    TestProvenance,
    /// The worker produced more evidence than one complete review may accept.
    /// Retrying the same task cannot change that policy boundary, so this
    /// parks for a human to split or otherwise rescope the task after the
    /// attempt's real spend has been settled.
    ReviewInputTooLarge,
    /// The captured patch names a changed object whose content is not present
    /// in the review evidence (binary, suppressed diff, or gitlink).
    ReviewInputOpaque,
    ReviewFailed,
    /// The reviewer could not run — an environment failure, not a judgement
    /// on the change.
    ReviewUnavailable,
    /// A worker or reviewer hit a decision it should not make alone (§12).
    NeedsHuman,
    /// A human was asked to unblock the task and said no. Never produced by
    /// [`next_step`] — it is how a question resolves, not how an attempt fails.
    Declined,
    /// The engine died between an attempt starting and finishing, so nothing
    /// judged the code. Never produced by [`next_step`] either — replay
    /// synthesizes it for a dangling `attempt_started` and hands the task back
    /// to the scheduler still on the same rung. It appears in the ledger with
    /// unknown spend, because an attempt that really ran and really drained a
    /// pool must not vanish from the record just because we cannot price it.
    Interrupted,
}

/// Who the failure happened to. `Timeout` and `RateLimited` mean opposite
/// things depending on this: an implementer that timed out failed its attempt
/// (§19), while a reviewer that timed out told us nothing about the code.
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
    /// Human-facing summary, for reports and questions.
    pub reason: String,
    /// What the retry sends back to the agent: a gate log tail (§11.1) or the
    /// reviewer's `required_changes` (§11.2), verbatim.
    pub feedback: Option<String>,
}

impl AttemptFailure {
    /// A failure attributed to the worker — the common case.
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

    /// Re-attribute to the reviewer. The kind stays what actually happened
    /// (a rate limit is still a rate limit, and the capacity engine will want
    /// to know); the origin is what stops the implementer being blamed.
    pub fn from_reviewer(mut self) -> Self {
        self.origin = FailureOrigin::Reviewer;
        self
    }

    /// An environment problem rather than a verdict on the code. These defer
    /// instead of consuming an attempt (§19).
    pub fn is_outage(&self) -> bool {
        matches!(
            (self.kind, self.origin),
            (FailureKind::RateLimited | FailureKind::ReviewUnavailable, _)
                | (FailureKind::Timeout, FailureOrigin::Reviewer)
        )
    }
}

/// Fixed for a task by its resolved chain and the run's config.
#[derive(Debug, Clone, Copy)]
pub struct LadderPolicy {
    /// `attempts_per` for this task's kind (§10.1).
    pub attempts_per: u32,
    /// How many rungs the resolved chain has.
    pub rungs: usize,
    /// How many deferrals a task may take before the pool counts as down and
    /// the human is asked instead. Without the capacity engine there is no
    /// reset time to wait for, so this bound is what stops an exhausted pool
    /// spinning forever.
    pub max_defers: u32,
}

/// Where the task stands right now.
#[derive(Debug, Clone, Copy)]
pub struct LadderState {
    /// Index into the chain of the rung the failed attempt ran on.
    pub rung: usize,
    /// Attempts spent on this rung *including* the one that just failed.
    pub attempts_on_rung: u32,
    pub defers: u32,
    /// Whether the next attempt could resume this one's session: the adapter
    /// advertises `session_resume` (from `probe()`) and the attempt actually
    /// returned a session id.
    pub resumable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Feed the failure back and try again at the same tier. `resume` carries
    /// §14's consequence: a resumed retry keeps the working tree, so the
    /// *cumulative* diff is what gets re-gated.
    RetrySameRung { resume: bool },
    /// Next rung, fresh session, accumulated feedback (§11.4).
    Escalate,
    /// Try again later without spending an attempt.
    Defer,
    /// Park this task behind a question; the scheduler keeps draining
    /// everything else (invariant 6).
    AskHuman(QuestionKind),
    /// Terminal for this task.
    Fail,
}

/// What to do after one failed attempt.
/// Whether an attempt that ended this way spent one of its rung's
/// `attempts_per`.
///
/// **Total, and the whole of the rule: an attempt spends iff the worker ran and
/// produced work to judge.**
///
/// This is the single production implementation of the allowance decision, and
/// it is here because [`next_step`] is its only consumer — the two would
/// otherwise be a rule and a copy of a rule, free to disagree about whether a
/// task escalates.
///
/// # It is derived, and it is derived from the failure
///
/// `attempt_finished` records a `SettlementTransition` and an `AttemptRecord`,
/// and **nothing that states the allowance decision**. That is deliberate: a
/// recorded conclusion sitting beside the recorded fact it derives from is an
/// internal-disagreement channel inside one event. A schema-4 resume derives it
/// here, from `AttemptRecord.failure`, which the event carries.
///
/// Keyed on the failure and not on the transition, because `Parked` is **not
/// one cell**. The legacy engine reaches `Next::AskHuman` by four paths that
/// disagree with each other, and `spends_allowance_matches_every_legacy_park_path`
/// is the grid of them.
///
/// # The packet does not state this
///
/// Its only attempt-path citations are interruption — T-ATTEMPT's "append
/// `attempt_interrupted` (unknown spend, allowance refunded…)" — and, by
/// analogy, one merge-verification "no attempt burned". The rule below is the
/// legacy engine's, preserved under `invariants_preserved[1]`, and the G2 pass
/// carries it into the packet.
#[must_use]
pub fn spends_allowance(failure: Option<&AttemptFailure>) -> bool {
    let Some(failure) = failure else {
        // No failure: the worker ran, and its work was judged and accepted.
        return true;
    };

    // An outage is not a run that produced work. `next_step` defers rather than
    // escalating for exactly this reason — "Escalating here would move the task
    // to a pricier rung because a *pool* was busy, and retrying would burn
    // attempts on a run that never got a verdict."
    if failure.is_outage() {
        return false;
    }

    // Listed rather than defaulted, so a new `FailureKind` does not compile
    // until someone decides whether it spends. A default arm here would answer
    // a question nobody asked, in the direction that costs an operator a rung.
    match failure.kind {
        // "Asked for a human explicitly: the code was never judged, so nothing
        // is spent and nothing escalates." The agent declined to work.
        FailureKind::NeedsHuman => false,
        // No chain resolved, so no worker ran at all: "A task whose chain
        // resolved to nothing cannot be retried into existence."
        FailureKind::NoChain => false,
        // "The engine died between an attempt starting and finishing, so
        // nothing judged the code … hands the task back to the scheduler still
        // on the same rung." The one cell the packet states outright, and it
        // agrees: T-ATTEMPT's resume action is "append `attempt_interrupted`
        // (unknown spend, allowance refunded …)". Two independent sources, one
        // answer.
        FailureKind::Interrupted => false,
        // "A human was asked to unblock the task and said no … it is how a
        // question resolves, not how an attempt fails." Unreachable as an
        // attempt outcome — `next_step` never produces it — and answered
        // anyway, because a match that is total is what makes a new variant
        // stop the build instead of taking a default. No worker ran for it.
        FailureKind::Declined => false,
        // Every remaining kind is a completed run. `ReviewInputTooLarge` and
        // `ReviewInputOpaque` are the instructive pair — the diff could not be
        // judged, and it still spends, because "The worker ran, so the attempt
        // is spent and must stay in the ledger". The line is *the worker ran*,
        // not *a verdict was reached*.
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
    // Asked for a human explicitly: the code was never judged, so nothing is
    // spent and nothing escalates. Straight to a question (§12).
    if failure.kind == FailureKind::NeedsHuman {
        return Next::AskHuman(QuestionKind::Clarify);
    }

    // The worker ran, so the attempt is spent and must stay in the ledger, but
    // no amount of automatic retrying can make the same complete diff fit the
    // review contract. Ask for a scope decision instead of paying again.
    if matches!(
        failure.kind,
        FailureKind::ReviewInputTooLarge | FailureKind::ReviewInputOpaque
    ) {
        return Next::AskHuman(QuestionKind::Unblock);
    }

    // Outages defer. Escalating here would move the task to a pricier rung
    // because a *pool* was busy, and retrying would burn attempts on a run
    // that never got a verdict.
    if failure.is_outage() {
        return if state.defers < policy.max_defers {
            Next::Defer
        } else {
            // The pool stayed down across every deferral: that is now a
            // genuine blocker, and blockers go to the top rung.
            Next::AskHuman(QuestionKind::Unblock)
        };
    }

    // A task whose chain resolved to nothing cannot be retried into existence.
    if failure.kind == FailureKind::NoChain || policy.rungs == 0 {
        return Next::Fail;
    }

    // A real rejection of the work: spend an attempt.
    if state.attempts_on_rung < policy.attempts_per {
        return Next::RetrySameRung {
            resume: state.resumable,
        };
    }
    if state.rung + 1 < policy.rungs {
        return Next::Escalate;
    }
    // §11.4: chain exhausted — the human is the top rung.
    Next::AskHuman(QuestionKind::Unblock)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The four paths by which the legacy engine parks a task, one cell
    /// each, with the comment that defines each cell quoted verbatim.**
    ///
    /// `Parked` looks like one settlement and is four decisions. The grid
    /// exists so the principle — *an attempt spends iff the worker ran and
    /// produced work to judge* — cannot drift from the paths that define it:
    /// a future edit that makes the principle prettier and one of these cells
    /// wrong fails here, and the quoted comment beside it says which
    /// engine-behaviour it just changed.
    ///
    /// Every quotation is from `next_step` above or from
    /// `engine::attempt::review_failure`, in this repository, and is the
    /// authority under `invariants_preserved[1]` — the packet states none of
    /// it.
    #[test]
    fn spends_allowance_matches_every_legacy_park_path() {
        // (kind, origin, spends, the legacy comment that decides it).
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
                spends_allowance(Some(&failure)),
                spends,
                "{kind:?} must {} the rung's allowance — {why}",
                if spends { "spend" } else { "not spend" }
            );
        }
    }

    /// The fourth park path is chain exhaustion, and it is a cell about
    /// *arithmetic* rather than about a kind.
    ///
    /// `next_step` reaches `AskHuman(Unblock)` at the end only once
    /// `attempts_on_rung >= attempts_per` on the top rung — so the retries that
    /// got there already spent them, and the park adds nothing. Asserted
    /// through `next_step` itself rather than restated, because the claim is
    /// about that function's control flow and a restatement would be a second
    /// copy of it.
    #[test]
    fn chain_exhaustion_parks_only_after_the_allowance_was_already_spent() {
        let policy = LadderPolicy {
            rungs: 1,
            attempts_per: 2,
            max_defers: 1,
        };
        let rejection = AttemptFailure::new(FailureKind::ReviewFailed, "rejected");
        assert!(
            spends_allowance(Some(&rejection)),
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
        // §11.4 prefers session resume, but only where the adapter supports it
        // and a session actually came back; otherwise the retry starts fresh.
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
        // Last rung, attempts spent: nothing cheaper or stronger is left.
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
        // The whole point: a busy pool must not push the task up-tier or eat
        // its retries. Even on the last attempt of the last rung, it defers.
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
        // Step 6's rule, enforced by the ladder: a judge that could not run
        // says nothing about the code, so the implementer is not retried,
        // escalated, or blamed.
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
        // Same kind, opposite handling — this is why origin exists.
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
            // Not on the last attempt, not on the last rung: the ladder never
            // gets consulted, because nothing judged the code.
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
