//! The single production authority for **what command an invocation runs**.
//!
//! # Why this module exists
//!
//! Two engines now need the same answer. The legacy engine (schemas 1–3) has
//! always assembled a worker's command inline in [`super::attempt::run_attempt`]
//! and a gate's inline in [`crate::gates::ShellGate::check`]; the schema-4
//! driver needs the same three command sets **up front**, in an
//! [`super::topology::AttemptPlan`], because a plan is a value it appends
//! `attempt_started` from.
//!
//! Assembling them twice is this slice's dominant defect class. Of the findings
//! PR7's review rounds produced, the expensive ones were all one rule
//! implemented twice: three append-error protocols, two barrier witnesses, two
//! run-directory censuses, two disagreeing retry rules — and, in this slice's
//! own dispatch branch, two derivations of a task's predicted region that
//! disagreed on every glob (`84a3978`).
//!
//! # What is and is not extracted here
//!
//! **Minting a `CommandSpec` was never duplicated.** This crate has exactly two
//! production mints — [`crate::gates::ShellKind::spec`] for a shell command and
//! `agent::bin::Invocation::spec` for an agent one — and both already document
//! themselves as the single place. What was about to be duplicated is the
//! **selection of their inputs**: which prompt, which permissions file, which
//! timeout, which profile. That selection is what moved here, and it is what
//! `the_worker_command_has_one_production_assembler` pins.
//!
//! The gate's selection is **not** here, and deliberately: it is one expression
//! over data [`crate::gates::ShellGate`] already owns, so it lives on that type
//! as `ShellGate::command`. Putting it here would make `gates.rs` — which sits
//! below the engine — depend upward on the engine.

use std::path::Path;

use crate::agent::{AgentAdapter, TaskRun};
use crate::error::UpstrokeError;
use crate::ir::{Task, WorkerProfile};
use crate::rundir::RunPaths;
use crate::runner::CommandSpec;

use super::attempt::{RetryBrief, materialize_prompt};

/// Everything one worker invocation's command is derived from.
///
/// A struct rather than ten parameters, and every field is an input the legacy
/// engine already had at the call site this was lifted from — nothing here is
/// new, and nothing here is defaulted. A field this type invented would be a
/// field one engine could set and the other could not.
pub(crate) struct WorkerAssembly<'a> {
    /// The bound agent's adapter. Its `id` is the `AgentId` the request
    /// carries, and its `build` is the mint.
    pub(crate) adapter: &'a dyn AgentAdapter,
    /// The routing decision for this attempt: tier, model, effort, pool.
    pub(crate) profile: &'a WorkerProfile,
    /// The task, as the frozen plan records it.
    pub(crate) task: &'a Task,
    /// The gate command lines the worker is permitted to run, which the prompt
    /// quotes and the permissions file allows.
    pub(crate) gate_cmds: &'a [String],
    /// The run's directories: where the settings file and the artifacts go.
    pub(crate) paths: &'a RunPaths,
    /// The per-task file stem, and the attempt number. Together they name the
    /// settings file, so two attempts of one task never share one.
    pub(crate) stem: &'a str,
    /// Which attempt this is, from 1.
    pub(crate) attempt: u32,
    /// On a retry, what the earlier attempts said. `None` on the first.
    pub(crate) retry: Option<&'a RetryBrief>,
    /// The checkout the worker edits.
    pub(crate) workspace: &'a Path,
    /// The agent session to resume, when one is being resumed.
    pub(crate) resume_session: Option<String>,
}

impl WorkerAssembly<'_> {
    /// The command this worker invocation runs as.
    ///
    /// Permissions first, then the prompt, then the mint — in that order,
    /// because the permissions file's path is a field of the `TaskRun` the
    /// prompt travels in, and an adapter that reads permissions from argv reads
    /// it from there.
    ///
    /// The stdin payload is attached here rather than by the caller. It is the
    /// adapter's answer about its own `TaskRun`, so a caller that attached it
    /// would be a second place that had to know which adapters want one.
    ///
    /// # Errors
    ///
    /// Whatever `materialize_permissions` or `build` returns — a permissions
    /// file that cannot be written, or an agent binary that cannot be resolved.
    pub(crate) fn command(&self) -> Result<CommandSpec, UpstrokeError> {
        let settings_path = self.adapter.materialize_permissions(
            self.profile,
            self.gate_cmds,
            &self.paths.settings(),
            &format!("{}-{}", self.stem, self.attempt),
        )?;

        let task_run = TaskRun {
            prompt: materialize_prompt(
                self.task,
                self.gate_cmds,
                &self.paths.artifacts(),
                self.retry,
            ),
            profile: self.profile.clone(),
            workspace: self.workspace.to_path_buf(),
            gate_cmds: self.gate_cmds.to_vec(),
            resume_session: self.resume_session.clone(),
            settings_path,
        };

        Ok(self
            .adapter
            .build(&task_run)?
            .stdin(self.adapter.stdin_payload(&task_run).as_bytes().to_vec()))
    }
}
