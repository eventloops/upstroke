//! Sequential execution engine (DESIGN.md §14) and the verification ladder it
//! drives (§11.4, §12, §19).
//!
//! Pre-flight, run branch, then a scheduler that drains the task graph one
//! attempt at a time: agent run → engine-captured diff → gates with evidence
//! axes (§11.1) → read-only review with a structured verdict (§11.2) →
//! engine-owned commit. A failed attempt does not end the task — it feeds the
//! failure back to the same rung (resuming the session where the adapter
//! supports it), then escalates a rung on a fresh session with the accumulated
//! feedback, and finally asks a human, who is the top rung.
//!
//! The scheduler's defining property is invariant 6: **a question parks only
//! the tasks it affects.** Everything else keeps draining, and the run
//! hard-blocks only when the runnable frontier is empty and everything left is
//! waiting on an answer. That is the moment — and the only moment — a human is
//! asked.
//!
//! Every transition here is an event (invariant 4). The engine never mutates
//! run state directly: it appends to `events.jsonl` and folds the event back in
//! through [`RunState::apply`], the same function `resume` and `status` use to
//! rebuild state from the file. A live run and a replay of its own log
//! therefore cannot disagree — there is no second path for them to disagree
//! along. `report.json` is written from that state as a projection for humans;
//! nothing ever reads it back.

mod attempt;
mod coordinator;
mod options;
mod preflight;
mod report;
mod resume;

use crate::error::TactusError;

pub use options::{
    DEFAULT_ATTEMPT_TIMEOUT, DEFAULT_MAX_DEFERS, Harness, ResumeOptions, RunOptions,
};
pub use report::{PoolDrainRow, RunOutcome, RunReport, TaskReport, TaskRunStatus, topo_order};

// Re-exported so `engine::AdapterSource` still resolves for callers that
// reasonably think of it as the engine's seam.
pub use crate::agent::{AdapterSource, BuiltinAdapters};
pub use crate::events::{AttemptRecord, FailureRecord};
pub use crate::ladder::{AttemptFailure, FailureKind, FailureOrigin};

pub fn run(opts: &RunOptions) -> Result<RunReport, TactusError> {
    run_with(opts, &BuiltinAdapters)
}

pub fn run_with(opts: &RunOptions, adapters: &dyn AdapterSource) -> Result<RunReport, TactusError> {
    run_harness(opts, &Harness::new(adapters))
}

pub fn run_harness(opts: &RunOptions, harness: &Harness<'_>) -> Result<RunReport, TactusError> {
    coordinator::run_harness_inner(opts, harness).map(|(report, _)| report)
}

pub fn resume(opts: &ResumeOptions) -> Result<RunReport, TactusError> {
    resume_with(opts, &BuiltinAdapters)
}

pub fn resume_with(
    opts: &ResumeOptions,
    adapters: &dyn AdapterSource,
) -> Result<RunReport, TactusError> {
    resume_harness(opts, &Harness::new(adapters))
}

/// §15: replay, verify the run branch still matches the record, re-probe, and
/// continue — parked questions intact.
///
/// Every refusal below exists because continuing would produce a *wrong*
/// result rather than merely an awkward one, and each says which of the four
/// things moved — the run, the plan, the config, or the branch — because that
/// is what decides what the operator does next.
///
/// Note what is *not* a refusal: gates that resolve differently today. Those
/// are taken from the record and run, so there is nothing to refuse — the
/// difference is a warning about an edit that does not apply here. A refusal is
/// for the cases where continuing would be wrong, and continuing under the
/// gates this run has been using all along is exactly right.
pub fn resume_harness(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<RunReport, TactusError> {
    resume::resume_harness_inner(opts, harness).map(|(report, _)| report)
}

#[cfg(test)]
mod tests;
