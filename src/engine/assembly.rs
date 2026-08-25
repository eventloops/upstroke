//! The single production authority for **what an invocation runs, and as what**.
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
use crate::ir::{Effort, PermissionMode, Task, Tier, WorkerProfile};
use crate::rundir::RunPaths;
use crate::runner::CommandSpec;

use super::attempt::{RetryBrief, materialize_prompt};

/// What the worker's prompt reads about the task.
///
/// Five fields, and [`materialize_prompt`] is the only thing in this path to
/// touch the task at all — the same narrowing `review::ReviewSubject` made, for
/// the same reason and against the same wall. The schema-4 driver holds a
/// `FrozenTaskSpec` from the frozen registry and no `ir::Task` anywhere, so
/// sharing the assembler would otherwise mean synthesising one: inventing an
/// id, a kind and a dependency list the prompt never reads. A conversion that
/// fabricates fields is free to drift from the plan it claims to represent.
///
/// Separate from `ReviewSubject` rather than one widened type, because the two
/// prompts genuinely read different things: a reviewer is handed artifacts
/// already resolved and never sees `artifacts_in`. Merging them would give each
/// caller fields it does not read, which is the wall this is climbing over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkerSubject<'a> {
    /// The task's one-line title.
    pub(crate) title: &'a str,
    /// Its body, which may be empty.
    pub(crate) body: &'a str,
    /// Its acceptance criteria.
    pub(crate) acceptance: &'a [String],
    /// Artifacts the prompt wires in as readable files.
    pub(crate) artifacts_in: &'a [crate::ir::ArtifactId],
    /// Artifacts the worker is asked to produce.
    pub(crate) artifacts_out: &'a [crate::ir::ArtifactId],
}

impl<'a> WorkerSubject<'a> {
    /// The subject of a legacy plan's task.
    #[must_use]
    pub(crate) fn of(task: &'a Task) -> Self {
        Self {
            title: &task.title,
            body: &task.body,
            acceptance: &task.acceptance,
            artifacts_in: &task.artifacts_in,
            artifacts_out: &task.artifacts_out,
        }
    }
}

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
    /// What the prompt reads about the task.
    pub(crate) task: WorkerSubject<'a>,
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

/// The routing facts an implementer's profile is built from.
///
/// **Ask for what you read.** [`implementer_profile`] reads exactly these four
/// and the pool; it never sees a chain, a task or a run. Naming them lets one
/// construction serve the legacy coordinator, which holds a
/// [`crate::route::Rung`] and resolves effort from the policy, and the schema-4
/// driver, which holds a [`crate::topology::events::RungBinding`] that already
/// carries the effort its run froze.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImplementerBinding<'a> {
    /// Which rung of the chain this is.
    pub(crate) tier: Tier,
    /// The agent whose CLI runs the work.
    pub(crate) agent: &'a str,
    /// The model it runs.
    pub(crate) model: &'a str,
    /// What this tier is worth on an agent with an effort axis.
    pub(crate) effort: Effort,
}

impl<'a> ImplementerBinding<'a> {
    /// A resolved chain's rung, with the effort the run's policy gives its tier.
    pub(crate) fn of_rung(rung: &'a crate::route::Rung, effort: Effort) -> Self {
        Self {
            tier: rung.tier,
            agent: &rung.binding.agent,
            model: &rung.binding.model,
            effort,
        }
    }
}

/// The profile one implementation attempt runs under.
///
/// The one production construction of an implementer's [`WorkerProfile`]. It
/// was inline in `coordinator.rs`, where the schema-4 driver could not reach
/// it; a driver that rebuilt it would be a second answer to `permissions`, and
/// a worker spawned `ReadyOnly` edits nothing while reporting success.
///
/// `pool` is passed rather than resolved here because resolving it needs the
/// run's config: §13 is read-only, so this is **attribution only** — which
/// subscription pays for the attempt, so the ledger and the estimator can say
/// so. Nothing routes on it.
pub(crate) fn implementer_profile(
    binding: ImplementerBinding<'_>,
    pool: Option<String>,
) -> WorkerProfile {
    WorkerProfile {
        name: format!("{}-{}", binding.tier, binding.model),
        agent: binding.agent.to_owned(),
        model: binding.model.to_owned(),
        pool: pool.unwrap_or_default(),
        permissions: PermissionMode::Edit,
        // What the rung's tier is worth on an agent with an effort axis:
        // without this the whole chain runs at one vendor default and
        // escalating a rung moves nothing (§10).
        effort: Some(binding.effort),
        max_turns: None,
        extra_args: Vec::new(),
    }
}
