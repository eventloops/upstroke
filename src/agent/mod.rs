//! Agent adapters (DESIGN.md §8, §16): turn a `TaskRun` into a subprocess of
//! an official agent CLI and parse what came back. Adapters never edit files,
//! never commit, and never speak HTTP — they only build commands and read
//! process output. One file per agent.

pub mod bin;
pub mod claude;
pub mod copilot;
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
    /// The gate commands this profile may run, and nothing else (§20). Empty
    /// for reviewers, which run nothing at all.
    ///
    /// Carried on the run rather than only handed to
    /// [`AgentAdapter::materialize_permissions`] because not every agent has a
    /// settings file to put them in: Copilot's permission surface is argv, so
    /// its `build` needs them at command-construction time.
    pub gate_cmds: Vec<String>,
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

    /// What to write to the child's stdin. Delivery is the adapter's call:
    /// CLIs that take the prompt as an argument instead return empty here.
    fn stdin_payload<'a>(&self, run: &'a TaskRun) -> &'a str {
        &run.prompt
    }

    /// Materialize this agent's permission surface (§20) into `dir`, returning
    /// the file the command should reference. Claude Code writes a settings
    /// JSON; Copilot will encode permissions as argv flags and write nothing.
    fn materialize_permissions(
        &self,
        _profile: &WorkerProfile,
        _gate_cmds: &[String],
        _dir: &std::path::Path,
        _stem: &str,
    ) -> Result<Option<PathBuf>, TactusError> {
        Ok(None)
    }
}

/// Registry in routing order; ids match `WorkerProfile.agent`.
pub static ADAPTERS: &[&dyn AgentAdapter] = &[&claude::ClaudeCodeAdapter, &copilot::CopilotAdapter];

pub fn by_id(id: &str) -> Option<&'static dyn AgentAdapter> {
    ADAPTERS.iter().copied().find(|a| a.id() == id)
}

/// Rate-limit signals are ground truth for the capacity engine (§13), so both
/// adapters read from one vocabulary rather than two that drift apart.
///
/// Phrases cover the subscription-window wording Claude Code prints ("5-hour
/// limit reached", "Weekly limit reached"), Copilot's credit and premium-request
/// wording (§13's two billing shapes), and API-level errors underneath either.
///
/// Only ever consulted for a FAILED attempt: a successful task *about* rate
/// limiting ("added backoff for 429 responses") must never be read as the pool
/// being exhausted, or verified work gets rolled back.
pub fn looks_rate_limited(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "usage limit",
        "rate limit",
        "rate_limit",
        "limit reached",
        "limit exceeded",
        "overloaded",
        "quota exceeded",
        "insufficient credits",
        "out of credits",
        "premium request",
        "monthly limit",
        "429",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_both_v0_1_adapters() {
        assert!(by_id("claude-code").is_some());
        assert!(by_id("copilot").is_some());
        assert!(by_id("aider").is_none(), "aider arrives in v0.2");
    }

    #[test]
    fn rate_limit_vocabulary_covers_both_vendors() {
        for phrase in [
            "5-hour limit reached ∙ resets 6pm",
            "Weekly limit reached",
            "API error: rate_limit_error",
            "You are out of credits for this month",
            "premium request allowance exhausted",
            "HTTP 429",
        ] {
            assert!(looks_rate_limited(phrase), "should signal: {phrase}");
        }
        assert!(!looks_rate_limited("wrote the pagination cursor encoder"));
    }
}
