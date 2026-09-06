//! Extended notes: `docs/internals/engine/mod.md`

mod assembly;
mod attempt;
mod classify;
mod coordinator;
mod options;
mod preflight;
mod report;
mod resume;
pub(crate) mod topology;

use crate::agent::proc::NoHooks;
use crate::error::UpstrokeError;
use crate::runner::Runner;
use crate::runner::host::{Contained, HostRunner, contain_write_command};

pub use options::{
    DEFAULT_ATTEMPT_TIMEOUT, DEFAULT_MAX_DEFERS, Harness, ResumeOptions, RunOptions,
};
pub use report::{PoolDrainRow, RunOutcome, RunReport, TaskReport, TaskRunStatus, topo_order};

pub use crate::agent::{AdapterSource, BuiltinAdapters};
pub use crate::events::{AttemptRecord, FailureRecord};
pub use crate::ladder::{AttemptFailure, FailureKind, FailureOrigin};

pub fn run(opts: &RunOptions) -> Result<RunReport, UpstrokeError> {
    run_with(opts, &BuiltinAdapters)
}

pub fn run_with(
    opts: &RunOptions,
    adapters: &dyn AdapterSource,
) -> Result<RunReport, UpstrokeError> {
    run_harness(opts, &Harness::new(adapters))
}

pub fn run_harness(opts: &RunOptions, harness: &Harness<'_>) -> Result<RunReport, UpstrokeError> {
    run_harness_on(opts, harness, &HostRunner::new())
}

fn run_harness_on(
    opts: &RunOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
) -> Result<RunReport, UpstrokeError> {
    run_contained(opts, harness, runner, || {
        contain_write_command(&mut NoHooks)
    })
}

fn run_contained(
    opts: &RunOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    contain: impl FnOnce() -> Result<Contained, UpstrokeError>,
) -> Result<RunReport, UpstrokeError> {
    let contained = contain()?;
    coordinator::run_harness_inner_on(opts, harness, runner, &contained).map(|(report, _)| report)
}

pub fn resume(opts: &ResumeOptions) -> Result<RunReport, UpstrokeError> {
    resume_with(opts, &BuiltinAdapters)
}

pub fn resume_with(
    opts: &ResumeOptions,
    adapters: &dyn AdapterSource,
) -> Result<RunReport, UpstrokeError> {
    resume_harness(opts, &Harness::new(adapters))
}

pub fn resume_harness(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
) -> Result<RunReport, UpstrokeError> {
    resume_harness_on(opts, harness, &HostRunner::new())
}

fn resume_harness_on(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
) -> Result<RunReport, UpstrokeError> {
    resume_contained(opts, harness, runner, || {
        contain_write_command(&mut NoHooks)
    })
}

fn resume_contained(
    opts: &ResumeOptions,
    harness: &Harness<'_>,
    runner: &dyn Runner,
    contain: impl FnOnce() -> Result<Contained, UpstrokeError>,
) -> Result<RunReport, UpstrokeError> {
    let contained = contain()?;
    resume::resume_harness_inner_on(opts, harness, runner, &contained).map(|(report, _)| report)
}

#[cfg(test)]
mod tests;
