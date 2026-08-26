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
use std::time::Duration;

use crate::agent::{AdapterSource, AgentAdapter, TaskRun};
use crate::error::UpstrokeError;
use crate::ir::{Effort, PermissionMode, Task, Tier, WorkerProfile};
use crate::rundir::RunPaths;
use crate::runner::{AgentId, CommandSpec};

use super::attempt::{RetryBrief, materialize_prompt, pool_option};
use super::topology::attempt::{
    AttemptPlan, AttemptPlans, GatePlan, InputsRequest, PlanRequest, ReviewInputs, ReviewerPlan,
};

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

    /// The subject of a frozen registry entry, which is what the schema-4
    /// driver holds. Deliberately a second constructor rather than a
    /// projection: `TaskEntry::to_task` exists, but building a whole `Task` to
    /// read five `&str` out of it allocates the other ten fields to throw them
    /// away.
    pub(crate) fn of_frozen(spec: &'a crate::topology::registry::FrozenTaskSpec) -> Self {
        Self {
            title: &spec.title,
            body: &spec.body,
            acceptance: &spec.acceptance,
            artifacts_in: &spec.artifacts_in,
            artifacts_out: &spec.artifacts_out,
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
    pub paths: &'a RunPaths,
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

    /// A frozen rung's binding, which already carries the effort its run
    /// resolved — the schema-4 driver never re-reads the policy.
    pub(crate) fn of_frozen(binding: &'a crate::topology::events::RungBinding) -> Self {
        Self {
            tier: binding.tier,
            agent: &binding.agent,
            model: &binding.model,
            effort: binding.effort,
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

/// The frozen registry's answer to [`AttemptPlans`], and the run's config
/// beside it.
///
/// **`pub`, and waiting for its production caller like everything else in the
/// schema-4 path.** `decisions.pr_sequence[8].production_effect` is "none
/// (TopologyPreview selector only)": `upstroke run` still drives the legacy
/// coordinator, so the only thing that builds a `RunSeams` today is a test.
/// PR12 activates the path and this is what it will construct.
///
/// Everything here is run-scoped: the gate set, the worker allowance, the pool
/// table, the CLI versions pre-flight probed. Nothing is per-attempt — that
/// arrives in the [`PlanRequest`], which is what makes one of these serve a
/// whole run.
pub struct FrozenPlans<'a> {
    /// Where an agent name becomes an adapter.
    pub adapters: &'a dyn AdapterSource,
    /// The run's directories — where permissions and artifacts live.
    pub paths: &'a RunPaths,
    /// The gate set, in the order the config wrote it.
    pub gates: &'a [crate::gates::ShellGate],
    /// The pool table §13 attributes spend against.
    pub pools: &'a [crate::capacity::Pool],
    /// What pre-flight certified each agent's CLI as.
    pub caps: &'a [(String, crate::agent::Caps)],
    /// How long one worker invocation may take.
    pub worker_timeout: Duration,
    /// The operator decisions a judge must honour, as the worker was given
    /// them.
    pub decisions: &'a [String],
}

impl FrozenPlans<'_> {
    /// What pre-flight certified this agent's CLI as, where it certified one.
    fn cli_version(&self, agent: &str) -> Option<String> {
        self.caps
            .iter()
            .find(|(name, _)| name == agent)
            .map(|(_, caps)| caps.version.clone())
    }
}

impl AttemptPlans for FrozenPlans<'_> {
    fn inputs(&self, request: &InputsRequest<'_>) -> Result<ReviewInputs, UpstrokeError> {
        let entry = request.entry;
        Ok(ReviewInputs {
            title: entry.spec.title.clone(),
            body: entry.spec.body.clone(),
            acceptance: entry.spec.acceptance.clone(),
            diff: request.diff.clone(),
            // Through the one production resolver, which reads the same two
            // artifact lists the worker's prompt wired to real files.
            artifacts: super::attempt::load_artifacts(
                &self.paths.artifacts(),
                WorkerSubject::of_frozen(&entry.spec),
            ),
            decisions: self.decisions.to_vec(),
            stem: entry.display_id.as_str().to_owned(),
        })
    }

    fn plan(&self, request: &PlanRequest<'_>) -> Result<AttemptPlan, UpstrokeError> {
        let entry = request.entry;
        let profile = implementer_profile(
            ImplementerBinding::of_frozen(&request.binding),
            crate::capacity::pool_for(&request.binding.agent, self.pools)
                .map(|pool| pool.name.clone()),
        );
        let adapter = self
            .adapters
            .get(&profile.agent)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!("no adapter registered for agent `{}`", profile.agent),
            })?;

        // The cmdlines, not the specs: this is what the worker's prompt quotes
        // as the bar it has to clear, and it is the same list the gate plans
        // below turn into commands.
        let gate_cmds: Vec<String> = self.gates.iter().map(|gate| gate.cmd.clone()).collect();

        let brief = (!request.feedback.is_empty()).then(|| RetryBrief {
            resumed: request.resume_session.is_some(),
            feedback: request.feedback.clone(),
        });
        let worker = WorkerAssembly {
            adapter,
            profile: &profile,
            task: WorkerSubject::of_frozen(&entry.spec),
            gate_cmds: &gate_cmds,
            paths: self.paths,
            stem: entry.display_id.as_str(),
            attempt: request.attempt.0,
            // §11.4's brief, when there is one. A first dispatch has no
            // feedback and passes `None`; a retry passes what the attempts
            // before it failed on.
            retry: brief.as_ref(),
            workspace: request.workspace,
            resume_session: request.resume_session.as_ref().map(|id| id.0.clone()),
        }
        .command()?;

        // Through `ShellGate::command`, the one production place a gate's
        // cmdline becomes a spec.
        let gates = self
            .gates
            .iter()
            .map(|gate| {
                let (command, timeout) = gate.command();
                GatePlan {
                    name: gate.name.clone(),
                    command,
                    timeout,
                }
            })
            .collect();

        let implementer =
            crate::review::PassBinding::new(&request.binding.agent, &request.binding.model);
        let pass_timeout = Duration::from_secs(entry.reviews.pass_timeout_secs);
        let reviewers = if entry.reviews.enabled {
            crate::review::passes_for(
                crate::review::ReviewBindings {
                    primary: entry.reviews.primary.as_ref(),
                    alternative: entry.reviews.alternative.as_ref(),
                    second_opinion: entry.reviews.second_opinion.as_ref(),
                },
                &implementer,
            )
            .into_iter()
            .map(|pass| ReviewerPlan {
                agent: AgentId::new(&pass.binding.agent),
                preflight_cli_version: self.cli_version(&pass.binding.agent),
                // **The reviewer's effort, from §10's own review axis.** This
                // said exactly that and then passed `request.binding.effort` —
                // the *implementer's*, the rung the work ran at. A comment
                // asserting the opposite of its line is worse than no comment:
                // it answers the question a reader would otherwise ask.
                //
                // `ResolvedEffortPolicy::review` is the axis, frozen on the
                // entry beside the implementation efforts.
                profile: pass.profile(entry.ladder.effort.review),
                lens: pass.lens,
                timeout: pass_timeout,
            })
            .collect()
        } else {
            Vec::new()
        };

        Ok(AttemptPlan {
            attempt: request.attempt,
            rung: request.rung,
            binding: request.binding.clone(),
            pool: pool_option(&profile.pool),
            resume_session: request.resume_session.clone(),
            materialization_observed: request.materialization_observed,
            agent: AgentId::new(&profile.agent),
            session_resume: self
                .caps
                .iter()
                .find(|(name, _)| name == &profile.agent)
                .is_some_and(|(_, caps)| caps.session_resume),
            worker,
            worker_timeout: self.worker_timeout,
            gates,
            reviewers,
        })
    }
}
