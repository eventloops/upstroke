//! Extended notes: `docs/internals/engine/assembly.md`

#![cfg_attr(not(test), allow(dead_code))]

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkerSubject<'a> {
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) acceptance: &'a [String],
    pub(crate) artifacts_in: &'a [crate::ir::ArtifactId],
    pub(crate) artifacts_out: &'a [crate::ir::ArtifactId],
}

impl<'a> WorkerSubject<'a> {
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

pub(crate) struct WorkerAssembly<'a> {
    pub(crate) adapter: &'a dyn AgentAdapter,
    pub(crate) profile: &'a WorkerProfile,
    pub(crate) task: WorkerSubject<'a>,
    pub(crate) gate_cmds: &'a [String],
    pub paths: &'a RunPaths,
    pub(crate) stem: &'a str,
    pub(crate) attempt: u32,
    pub(crate) retry: Option<&'a RetryBrief>,
    pub(crate) workspace: &'a Path,
    pub(crate) resume_session: Option<String>,
}

impl WorkerAssembly<'_> {
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImplementerBinding<'a> {
    pub(crate) tier: Tier,
    pub(crate) agent: &'a str,
    pub(crate) model: &'a str,
    pub(crate) effort: Effort,
}

impl<'a> ImplementerBinding<'a> {
    pub(crate) fn of_rung(rung: &'a crate::route::Rung, effort: Effort) -> Self {
        Self {
            tier: rung.tier,
            agent: &rung.binding.agent,
            model: &rung.binding.model,
            effort,
        }
    }

    pub(crate) fn of_frozen(binding: &'a crate::topology::events::RungBinding) -> Self {
        Self {
            tier: binding.tier,
            agent: &binding.agent,
            model: &binding.model,
            effort: binding.effort,
        }
    }
}

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
        effort: Some(binding.effort),
        max_turns: None,
        extra_args: Vec::new(),
    }
}

pub struct FrozenPlans<'a> {
    pub adapters: &'a dyn AdapterSource,
    pub paths: &'a RunPaths,
    pub gates: &'a [crate::gates::ShellGate],
    pub pools: &'a [crate::capacity::Pool],
    pub caps: &'a [(String, crate::agent::Caps)],
    pub worker_timeout: Duration,
    pub decisions: &'a [String],
}

impl FrozenPlans<'_> {
    fn cli_version(&self, agent: &str) -> Option<String> {
        self.caps
            .iter()
            .find(|(name, _)| name == agent)
            .map(|(_, caps)| caps.version.clone())
    }
}

impl AttemptPlans for FrozenPlans<'_> {
    fn pool_for(&self, agent: &str) -> Option<String> {
        crate::capacity::pool_for(agent, self.pools).map(|pool| pool.name.clone())
    }

    fn inputs(&self, request: &InputsRequest<'_>) -> Result<ReviewInputs, UpstrokeError> {
        let entry = request.entry;
        Ok(ReviewInputs {
            title: entry.spec.title.clone(),
            body: entry.spec.body.clone(),
            acceptance: entry.spec.acceptance.clone(),
            diff: request.diff.clone(),
            artifacts: super::attempt::load_artifacts(
                &self.paths.artifacts(),
                WorkerSubject::of_frozen(&entry.spec),
            ),
            decisions: self.decisions.to_vec(),
            stem: crate::util::filename_component(entry.display_id.as_str()),
        })
    }

    fn plan(&self, request: &PlanRequest<'_>) -> Result<AttemptPlan, UpstrokeError> {
        let entry = request.entry;
        let profile = implementer_profile(
            ImplementerBinding::of_frozen(&request.binding),
            self.pool_for(&request.binding.agent),
        );
        let adapter = self
            .adapters
            .get(&profile.agent)
            .ok_or_else(|| UpstrokeError::Agent {
                message: format!("no adapter registered for agent `{}`", profile.agent),
            })?;

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
            stem: &crate::util::filename_component(entry.display_id.as_str()),
            attempt: request.attempt.0,
            retry: brief.as_ref(),
            workspace: request.workspace,
            resume_session: request.resume_session.as_ref().map(|id| id.0.clone()),
        }
        .command()?;

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
        let reviewers = if let Some(bindings) = entry.reviews.bindings() {
            crate::review::passes_for(bindings, &implementer)
                .into_iter()
                .map(|pass| ReviewerPlan {
                    agent: AgentId::new(&pass.binding.agent),
                    preflight_cli_version: self.cli_version(&pass.binding.agent),
                    profile: {
                        let mut profile = pass.profile(entry.ladder.effort.review);
                        profile.pool = self.pool_for(&pass.binding.agent).unwrap_or_default();
                        profile
                    },
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
