//! Extended notes: `docs/internals/agent/proc/ambient.md`

#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use crate::error::UpstrokeError;
#[cfg(windows)]
use crate::topology::effects::{InjectionMode, SubEffectPoint};

use super::SpawnHooks;
#[cfg(windows)]
use super::hooks::apply;
#[cfg(windows)]
use super::windows_job;

#[cfg(windows)]
pub fn join_ambient_job(hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {
    join_ambient_job_with(hooks, windows_job::join_ambient)
}

#[cfg(windows)]
pub(super) fn join_ambient_job_with(
    hooks: &mut dyn SpawnHooks,
    join: impl FnOnce() -> Result<(), String>,
) -> Result<(), UpstrokeError> {
    apply(
        hooks.point_mode(SubEffectPoint::AmbientJobJoined, InjectionMode::ErrorReturn),
        SubEffectPoint::AmbientJobJoined,
    )
    .map_err(|_| UpstrokeError::Refused {
        message: AMBIENT_REFUSAL_PREFIX.to_owned() + AMBIENT_REFUSAL_SIMULATED,
    })?;
    join().map_err(|message| UpstrokeError::Refused {
        message: format!("{AMBIENT_REFUSAL_PREFIX}{message}. No process was spawned"),
    })?;
    apply(
        hooks.point_mode(SubEffectPoint::AmbientJobJoined, InjectionMode::Kill),
        SubEffectPoint::AmbientJobJoined,
    )
}

#[cfg(not(windows))]
pub fn join_ambient_job(_hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {
    Ok(())
}

pub const AMBIENT_REFUSAL_PREFIX: &str = concat!(
    "cannot start a write command: on Windows every child must be a member of ",
    "the coordinator's ambient kill-on-close Job Object from creation ",
    "(INV-18), and "
);

pub const AMBIENT_REFUSAL_SIMULATED: &str = concat!(
    "the ambient Job Object could not be established (simulated failure). ",
    "No process was spawned"
);

#[cfg(windows)]
#[must_use]
pub fn process_alive(pid: u32, creation_time: u64) -> bool {
    windows_job::process_alive(pid, creation_time)
}

#[cfg(windows)]
#[must_use]
pub fn process_creation_time(pid: u32) -> Option<u64> {
    windows_job::process_creation_time(pid)
}

#[cfg(windows)]
#[must_use]
pub fn ambient_job_established() -> bool {
    windows_job::ambient_established()
}

#[cfg(windows)]
#[must_use]
pub fn child_in_ambient_job(pid: u32) -> Option<bool> {
    windows_job::ambient_contains(pid)
}

#[cfg(all(windows, test))]
pub(crate) fn poison_ambient_for_tests(message: &str) -> bool {
    windows_job::poison_ambient_for_tests(message)
}

pub fn set_container_reclaim_scope(
    scope: Option<&crate::runner::container::census::ReaperContainerScope>,
) -> Result<(), UpstrokeError> {
    #[cfg(unix)]
    {
        super::termination::set_container_reclaim_scope(scope)
    }
    #[cfg(not(unix))]
    {
        let _ = scope;
        Ok(())
    }
}
