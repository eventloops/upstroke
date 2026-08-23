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

pub mod candidate;
pub mod emit;
pub mod identity;
pub mod seams;
pub mod select;
pub mod settle;
pub mod startup;

pub use candidate::{
    CandidateJournal, CandidateNames, CandidateRecovery, JudgedTree, OrphanPin, PinnedCandidate,
    PromotingCandidate, QueuedCandidate, UnpinnedCandidate,
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
pub use select::{Admitted, Breach, Ceiling, Spend, Step, checkpoint, select};
pub use settle::{
    Deferral, FinishedAttempt, ManagedWorktrees, RetryOutcome, RetryRequest, Settled,
    WorktreeVerify, close_generation, close_retained, rematerialize_question, retry, run_ending,
    settle_failed, settle_succeeded,
};
pub use startup::{
    BarrierHeld, CensusInputs, FreshCensused, ResumeCensused, RunDirCensusReport, RunDirEntry,
    RunDirOutcome, StartupCensus, WorktreeLocked, resume_census, startup_census,
};
