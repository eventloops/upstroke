//! The single production authority for **what an observation means about an
//! attempt**.
//!
//! # Why this module exists
//!
//! `ladder::next_step` reads an [`AttemptFailure`] and decides the whole of
//! what happens to a task: retry the rung, escalate, defer, park, or fail. It
//! is also what the **allowance decision** is derived from — an attempt spends
//! one of its rung's `attempts_per` iff the worker ran and produced work to
//! judge, and `AttemptRecord.failure` is the field that says which happened.
//!
//! So a second engine classifying the same observations its own way would be
//! two rules deciding one thing, and the disagreement would not surface as a
//! wrong answer — it would surface as a task escalating to a pricier tier
//! because the other engine called the same diff something else. That is the
//! shape of `84a3978`, one field over.
//!
//! # What is here, and what deliberately is not
//!
//! Here: the classifications that were **inline** in the legacy engine's
//! verification ladder — the diff's own problems, and a failed gate.
//!
//! Not here, because they were already functions and already reachable:
//! `attempt::review_failure`, which turns a `ReviewResult` into an
//! `AttemptFailure`, and `attempt::pool_option`. A "pure move" of something
//! that already sits alone in a callable function is churn, not extraction.
//!
//! Not here either: the **running**. Gates and reviews execute at each
//! engine's own point in its own order, and only the *decision* about what
//! their result means is shared — the same split `ShellGate::command` makes
//! for a gate's command.

use crate::events::{ReviewPassOutcome, ReviewRecord};
use crate::gates::{self, GateFailure};
use crate::ir::TaskKind;
use crate::ladder::{AttemptFailure, FailureKind};
use crate::review;

/// What the diff alone says is wrong with an attempt, before anything runs.
///
/// Two observations, in the legacy engine's order, and the order is the rule:
/// a diff no reviewer can read is refused before a Test task's provenance is
/// checked, because provenance is a judgement about content and the first
/// finding is that the content cannot be judged at all.
///
/// `has_reviewers` is load-bearing rather than a convenience. A diff that is
/// merely **too large** is only a failure when something is going to review it;
/// with reviews disabled there is no reader to defeat, and refusing it would
/// fail an attempt for a rule the run has switched off. An **opaque** diff
/// fails either way, because the engine's own capture could not read it.
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

/// What a failed gate says about an attempt.
///
/// The log tail rides as feedback because the next attempt's prompt quotes it:
/// a gate that failed without saying what it printed asks the worker to guess.
#[must_use]
pub(crate) fn gate_failure(failure: &GateFailure) -> AttemptFailure {
    AttemptFailure::new(
        FailureKind::GateFailed,
        format!("gate `{}` failed: {}", failure.gate, failure.summary),
    )
    .with_feedback(failure.log_tail.clone())
}

/// What one review pass is recorded as, and what it is recorded from.
///
/// A struct rather than ten parameters, the same shape
/// `assembly::WorkerAssembly` takes and for the same reason: every field is
/// something the caller already holds, and one this type invented would be a
/// field one engine could set and the other could not.
pub(crate) struct ReviewPassFacts<'a> {
    /// The lens's name — which review this was.
    pub(crate) pass: &'a str,
    /// The agent that judged, and the model it ran.
    pub(crate) agent: &'a str,
    /// The model, which `AttemptRecord` requires as a `String` and not an
    /// `Option`.
    pub(crate) model: &'a str,
    /// The adapter that built the invocation.
    pub(crate) adapter: &'a str,
    /// What pre-flight certified this CLI as, where it certified one.
    pub(crate) preflight_cli_version: Option<String>,
    /// The effort the routing decision bound.
    pub(crate) effort: Option<crate::ir::Effort>,
    /// The pool, where the agent takes one.
    pub(crate) pool: Option<String>,
    /// What the vendor said this cost, where it says.
    pub(crate) cost_usd: Option<f64>,
    /// Whether the judge never ran.
    pub(crate) unavailable: bool,
    /// Whether it ran and said no.
    pub(crate) failed: bool,
}

impl ReviewPassFacts<'_> {
    /// The record.
    ///
    /// **`unavailable` and `failed` are different questions and this keeps
    /// them apart.** A judge that never ran is not a judge that said no, and
    /// the ledger has to show which happened — a reviewer counted as having
    /// rejected the work when its CLI was rate-limited would spend an attempt
    /// for an outage, and `ladder::next_step` defers an outage precisely so
    /// that it does not.
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
