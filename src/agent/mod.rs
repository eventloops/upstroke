//! Agent adapters (DESIGN.md §8, §16): turn a `TaskRun` into a subprocess of
//! an official agent CLI and parse what came back. Adapters never edit files,
//! never commit, and never speak HTTP — they only build commands and read
//! process output. One file per agent.

pub mod claude;
pub mod proc;

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::TactusError;
use crate::ir::{Outcome, WorkerProfile};

pub use proc::ProcessOutput;

/// Capabilities discovered by `probe()` at pre-flight (§14). Copilot's CLI
/// has shipped breaking flag removals, so capability probing is load-bearing,
/// not decorative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caps {
    /// Version string as reported by the binary, best-effort.
    pub version: String,
    pub json_output: bool,
    pub session_resume: bool,
    pub cost_reporting: bool,
    pub read_only_mode: bool,
    pub acp: bool,
    pub model_list: bool,
}

/// Everything an adapter needs to build one attempt's subprocess. The engine
/// materializes the prompt (§14: body + acceptance + artifacts + conventions
/// brief) — adapters never re-derive it.
#[derive(Debug, Clone)]
pub struct TaskRun {
    /// Fully materialized prompt, delivered on stdin.
    pub prompt: String,
    pub profile: WorkerProfile,
    /// Working directory for the subprocess (the workspace repo root).
    pub workspace: PathBuf,
    /// Same-rung retry: resume this session with feedback instead of starting
    /// fresh (§11.4).
    pub resume_session: Option<String>,
    /// Per-run permission settings file, materialized by the engine from
    /// [`claude::permission_settings`]-style generators (§20).
    pub settings_path: Option<PathBuf>,
}

/// DESIGN.md §8 `AgentAdapter`.
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    /// Locate the binary and report version + capabilities. Ran at pre-flight;
    /// a missing binary is a refusal to start, not a task failure (§19).
    fn probe(&self) -> Result<Caps, TactusError>;
    fn build(&self, run: &TaskRun) -> Result<Command, TactusError>;
    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError>;
}

/// Registry in routing order; ids match `WorkerProfile.agent`.
pub static ADAPTERS: &[&dyn AgentAdapter] = &[&claude::ClaudeCodeAdapter];

pub fn by_id(id: &str) -> Option<&'static dyn AgentAdapter> {
    ADAPTERS.iter().copied().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_claude_code() {
        assert!(by_id("claude-code").is_some());
        assert!(by_id("copilot").is_none(), "copilot arrives in step 9");
    }
}
