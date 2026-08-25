//! `TopologyRun` — the schema-4 run lifecycle.
//!
//! `decisions.sequential_substrate.engine`: "`src/engine/topology.rs`
//! TopologyRun drives schema 4 at max_parallel = 1 synchronously; every path
//! exists here before Tokio."
//!
//! This module is the **conductor** of a schema-4 run. Almost nothing here is
//! new machinery: the event funnel and its stable-prefix barrier, the git
//! funnels of [`crate::workspace_manager`], the container census, and the
//! private-half ownership proof were all built and tested by PR4–PR6 and are
//! waiting for their first production caller. What arrives here is the
//! *ordering* over them — P0–P8, the recovery order, the candidate sequence —
//! and orderings are where this project's defects actually live: of 29
//! classified findings across PR3–PR6, `wrong_internal_assumption` is 48.3%,
//! three times `wrong_external_fact`.
//!
//! That measurement is the argument for the shape this module is being built
//! into: a rule it could get wrong at runtime is preferably a rule the type
//! checker refuses to compile. Three such rules are **planned and not yet
//! written** — a recovery order that is a chain of witnesses each consuming the
//! last, a creator deletion boundary minted only from the outcome of the step
//! that could have crossed it, and P0–P8 as a typestate. None of them exists
//! here today; what exists is the seams and the identity ledgers they will be
//! built on.
//!
//! Stated in the future tense deliberately. The device is established —
//! [`crate::rundir::PrivateHalfProof`],
//! [`crate::runner::container::intent::IntentWritten`] and
//! [`crate::topology::fold::TopologyDelta`] all use it — but a module comment
//! that describes an intended shape as an existing one is a false claim about
//! the code, and this project treats that as a defect rather than as
//! optimism.
//!
//! # Nothing here is a production path yet
//!
//! `decisions.pr_sequence[8].production_effect` is "none (TopologyPreview
//! selector only)". `upstroke run` still writes schema 3 and still drives
//! [`super::coordinator`]; a schema-4 run is reachable only from a
//! `#[cfg(test)]` writer selector. PR12 activates it.

pub mod attempt;
pub mod candidate;
pub mod create;
pub mod dispatch;
pub mod emit;
pub mod identity;
pub mod prelock;
pub mod seams;
pub mod select;
pub mod settle;
pub(crate) mod startup;

/// The schema-4 attempt-plan assembler, which lives engine-side.
///
/// Re-exported here rather than from the `engine` facade, whose contents are
/// enumerated by the packet and asserted by
/// `the_engine_facade_exposes_exactly_the_items_the_packet_enumerates`. This is
/// schema-4 vocabulary, and this module is where the schema-4 vocabulary is.
///
/// It is engine-side rather than here for the reason [`attempt::AttemptPlans`]
/// exists at all: building a plan materializes the permissions file that
/// defines the attempt's sandbox, and a topology module may not be allowlisted
/// for that write.
pub use super::assembly::FrozenPlans;

pub use attempt::{
    AttemptContext, AttemptOutcome, AttemptPlan, AttemptRun, Capture, GatePlan, Judgement,
    ReviewerPlan, Verdict,
};
pub use candidate::{
    CandidateJournal, CandidateNames, CandidateRecovery, JudgedTree, OrphanPin, PinnedCandidate,
    PromotingCandidate, QueuedCandidate, UnpinnedCandidate,
};
pub use dispatch::{
    DispatchKind, DispatchRequest, Dispatched, EventEmitter, Reuse, close_at_run_end, dispatch,
    materialize_repair, resume_open_no_attempt, task_slot, verify_or_recreate, verify_reuse,
};
pub use emit::{
    AppendError, AppendOutcome, EmitError, EmitState, FirstAppendDisposition, RunIdentity, emit,
};
pub use identity::{
    AttemptIdentities, InvocationLedger, PreflightIdentities, ReservationKind, Reservations,
    SequenceIdentities, SlotAssertion, SlotPair, is_slotted,
};
pub use seams::{
    HarnessTopologyHooks, IdSource, NoTopologyHooks, RealIds, SystemClock, TimeSource,
    TopologyHooks,
};
pub mod preflight;
pub mod recover;
pub mod run;

pub use preflight::{Probed, RunPreflight};
pub use recover::{
    BarrierHeld, EmitContext, LocksHeld, PreflightCertified, RecordsVerified, ResumeCensused,
    Resumed, RootDerived, RunnerRebuilt,
};
pub use run::{Disposition, LoopBranch};
pub use select::{Admitted, Breach, Ceiling, Spend, Step, checkpoint, select};
pub use settle::{
    Deferral, FinishedAttempt, ManagedWorktrees, RetryOutcome, RetryRequest, Settled,
    WorktreeVerify, close_generation, close_retained, rematerialize_question, retry, run_ending,
    settle_failed, settle_succeeded,
};
pub use startup::{
    CensusInputs, FailedStep, FreshCensused, RunDirCensusReport, RunDirEntry, RunDirOutcome,
    StartupCensus, WorktreeLocked, startup_census,
};

/// The schema-4 run that `dispatch.rs` and `attempt.rs` are tested against.
///
/// One fixture for both, because the two halves of one lifecycle are tested
/// against one run: an attempt test needs a dispatched generation and a
/// dispatch test needs the attempt that never started. Declared here rather
/// than under either module for the same reason.
///
/// **Private, and declared last.** Private because a module's own descendants
/// can see it — `dispatch::tests` and `attempt::tests` both are — so a `pub`
/// qualifier would widen it for nothing and would stop the two source censuses
/// that recognise a whole-file test module by the literal `#[cfg(test)] mod `
/// from recognising this one. Last because `effects::production_region` cuts a
/// file at its first `#[cfg(test)]`, and a declaration higher up would take the
/// re-exports above it out of every census that reads that region.
#[cfg(test)]
mod scaffold;
