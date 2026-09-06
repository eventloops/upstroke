//! Extended notes: `docs/internals/engine/options.md`

use std::path::PathBuf;
use std::time::Duration;

use crate::agent::AdapterSource;
#[cfg(test)]
use crate::error::UpstrokeError;
use crate::interaction::{self, AnswerSource, InteractionMode, Sleeper};
use crate::rundir::RunPaths;
#[cfg(test)]
use crate::workspace::Workspace;

pub const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub const DEFAULT_MAX_DEFERS: u32 = 3;

#[cfg(test)]
pub(super) type AfterCandidateCapture =
    fn(&Workspace, &crate::workspace::CapturedCandidate) -> Result<(), UpstrokeError>;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    pub repo_root: PathBuf,
    pub attempt_timeout: Duration,
    pub interaction: Option<InteractionMode>,
    pub defer_backoff: Duration,
    pub max_defers: u32,
    pub private_root: Option<PathBuf>,
    pub wait_on_block: Option<Duration>,
    pub budget_usd: Option<f64>,
    #[cfg(test)]
    pub(super) after_candidate_capture: Option<AfterCandidateCapture>,
    #[cfg(test)]
    pub(super) log_hooks: Option<fn() -> Box<dyn crate::events::log::EventHooks>>,
}

impl RunOptions {
    pub fn new(plan_path: PathBuf, repo_root: PathBuf) -> Self {
        Self {
            plan_path,
            config_path: None,
            pools_path: None,
            repo_root,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            interaction: None,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
            private_root: None,
            wait_on_block: None,
            budget_usd: None,
            #[cfg(test)]
            after_candidate_capture: None,
            #[cfg(test)]
            log_hooks: None,
        }
    }

    pub(super) fn paths(&self, run_id: &str) -> RunPaths {
        match &self.private_root {
            Some(root) => RunPaths::with_private_root(&self.repo_root, run_id, root),
            None => RunPaths::new(&self.repo_root, run_id),
        }
    }
}

pub struct Harness<'a> {
    pub adapters: &'a dyn AdapterSource,
    pub answers: Option<&'a dyn AnswerSource>,
    pub sleeper: Option<&'a dyn Sleeper>,
}

impl<'a> Harness<'a> {
    pub fn new(adapters: &'a dyn AdapterSource) -> Self {
        Self {
            adapters,
            answers: None,
            sleeper: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResumeOptions {
    pub run_id: String,
    pub repo_root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub pools_path: Option<PathBuf>,
    pub interaction: Option<InteractionMode>,
    pub attempt_timeout: Duration,
    pub defer_backoff: Duration,
    pub max_defers: u32,
    pub private_root: Option<PathBuf>,
    pub wait_on_block: Option<Duration>,
    pub budget_usd: Option<f64>,
}

impl ResumeOptions {
    pub fn new(run_id: String, repo_root: PathBuf) -> Self {
        Self {
            run_id,
            repo_root,
            config_path: None,
            pools_path: None,
            interaction: None,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
            defer_backoff: interaction::DEFAULT_DEFER_BACKOFF,
            max_defers: DEFAULT_MAX_DEFERS,
            private_root: None,
            wait_on_block: None,
            budget_usd: None,
        }
    }
}
