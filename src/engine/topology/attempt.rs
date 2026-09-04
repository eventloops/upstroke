//! Extended notes: `docs/internals/engine/topology/attempt.md`

use std::path::PathBuf;

use crate::agent::AdapterSource;
use crate::agent::proc::ProcessOutput;
use crate::engine::attempt::review_failure;
use crate::error::UpstrokeError;
use crate::events::ReviewRecord;
use crate::gates::GateFailure;
use crate::ir::WorkerProfile;
use crate::ladder::AttemptFailure;
use crate::review;
use crate::rundir::RunPaths;
use crate::runner::{
    AgentId, CommandSpec, InvocationId, Runner, RunnerRequest, gate_request, worker_request,
};
use crate::topology::events::{
    AttemptInterrupted4, AttemptNumber, AttemptStarted4, Materialization, RungBinding, SessionId,
    TopologyEventBody,
};
use crate::workspace_manager::{
    ObjectId, Slot, Snapshot, SnapshotInput, SnapshotName, WorkspaceManager,
};

use super::dispatch::{self, Dispatched, EventEmitter};
use super::identity::{AttemptIdentities, InvocationLedger, SlotAssertion, SlotPair, is_slotted};
use super::seams::TopologyHooks;

#[derive(Debug, Clone)]
pub struct ReviewerPlan {
    pub agent: AgentId,
    pub profile: WorkerProfile,
    pub lens: review::Lens,
    pub preflight_cli_version: Option<String>,
    pub timeout: std::time::Duration,
}

pub struct ReviewInputs {
    pub title: String,
    pub body: String,
    pub acceptance: Vec<String>,
    pub diff: String,
    pub artifacts: Vec<(String, String)>,
    pub decisions: Vec<String>,
    pub stem: String,
}

pub struct PlanRequest<'a> {
    #[allow(dead_code)]
    pub key: crate::topology::registry::TaskKey,
    pub entry: &'a crate::topology::registry::TaskEntry,
    pub attempt: AttemptNumber,
    pub rung: u32,
    pub binding: RungBinding,
    pub workspace: &'a std::path::Path,
    pub resume_session: Option<SessionId>,
    pub feedback: Vec<crate::events::Feedback>,
    pub materialization_observed: Option<Materialization>,
}

pub struct InputsRequest<'a> {
    pub entry: &'a crate::topology::registry::TaskEntry,
    pub diff: String,
}

pub trait AttemptPlans {
    fn inputs(&self, request: &InputsRequest<'_>) -> Result<ReviewInputs, UpstrokeError>;

    fn pool_for(&self, agent: &str) -> Option<String>;

    fn plan(&self, request: &PlanRequest<'_>) -> Result<AttemptPlan, UpstrokeError>;
}

pub trait ReviewInputPolicy {
    fn problem(
        &self,
        worktree: &std::path::Path,
        tree: &str,
    ) -> Result<Option<String>, UpstrokeError>;
}

pub trait ReviewPasses {
    fn run(
        &self,
        cx: &review::ReviewCx<'_>,
        runner: &dyn Runner,
        invocations: &review::ReviewInvocations,
    ) -> Result<review::ReviewOutcome, UpstrokeError>;
}

#[derive(Debug, Clone)]
pub struct AttemptPlan {
    pub attempt: AttemptNumber,
    pub rung: u32,
    pub binding: RungBinding,
    pub pool: Option<String>,
    pub resume_session: Option<SessionId>,
    pub materialization_observed: Option<Materialization>,
    pub agent: AgentId,
    pub session_resume: bool,
    pub worker: CommandSpec,
    pub worker_timeout: std::time::Duration,
    pub gates: Vec<GatePlan>,
    pub reviewers: Vec<ReviewerPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatePlan {
    pub name: String,
    pub command: CommandSpec,
    pub timeout: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct AttemptRun {
    pub identities: AttemptIdentities,
    pub worker: ProcessOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub tree: String,
    pub parent: String,
}

fn captured_object_id(source: &str, value: String) -> Result<ObjectId, UpstrokeError> {
    ObjectId::new(value).map_err(|refusal| UpstrokeError::Git {
        message: format!("{source} did not yield an object id: {refusal}"),
    })
}

#[derive(Debug, Clone)]
pub struct Assessment {
    pub outcome: crate::ir::Outcome,
    pub failure: Option<AttemptFailure>,
}

#[derive(Debug, Clone, Copy)]
pub struct AttemptSite<'a> {
    pub key: crate::topology::registry::TaskKey,
    pub generation: crate::topology::events::GenerationId,
    pub base: &'a crate::topology::events::CommitSha,
    pub slot: &'a Slot,
    pub worktree: &'a std::path::Path,
}

impl Dispatched {
    #[must_use]
    pub fn site(&self) -> AttemptSite<'_> {
        AttemptSite {
            key: self.key,
            generation: self.generation,
            base: &self.base,
            slot: &self.slot,
            worktree: &self.worktree,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Judging<'a> {
    pub run: &'a AttemptRun,
    pub capture: &'a Capture,
    pub assessed: &'a Assessment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub output_limited: bool,
    pub timed_out: bool,
    pub log: String,
    pub invocation: InvocationId,
    pub workspace: PathBuf,
    pub code: Option<i32>,
}

impl Verdict {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Debug, Clone)]
pub struct Judgement {
    pub gates: Vec<Verdict>,
    pub reviews: Vec<ReviewRecord>,
    pub failure: Option<AttemptFailure>,
}

impl Judgement {
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.failure.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Interrupted,
    Cancelled,
}

impl AttemptOutcome {
    const fn detail(self) -> &'static str {
        match self {
            Self::Interrupted => {
                "a coordinator died holding this attempt; the spend is unknown and nothing was \
                 judged"
            }
            Self::Cancelled => {
                "the run halted while this attempt was in flight; its invocations were cancelled \
                 and the spend is unknown"
            }
        }
    }
}

pub struct AttemptContext<'a> {
    pub manager: &'a WorkspaceManager,
    pub hooks: &'a mut dyn TopologyHooks,
    pub emitter: &'a mut dyn EventEmitter,
    pub runner: &'a dyn Runner,
    pub slots: &'a mut SlotAssertion,
    pub ledger: &'a mut InvocationLedger,
    pub adapters: &'a dyn AdapterSource,
    pub paths: &'a RunPaths,
    pub reviews: &'a dyn ReviewPasses,
    pub input_policy: &'a dyn ReviewInputPolicy,
}

impl AttemptContext<'_> {
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
        self.emitter
            .emit(body, self.hooks)
            .map_err(|failure| failure.discharging(self.ledger))
    }

    pub fn start(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
    ) -> Result<AttemptRun, UpstrokeError> {
        self.emit(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key: site.key,
                generation: site.generation,
                attempt: plan.attempt,
                rung: plan.rung,
                binding: plan.binding.clone(),
                pool: plan.pool.clone(),
                resume_session: plan.resume_session.clone(),
                materialization_observed: plan.materialization_observed,
            },
        })?;

        self.run_worker(site, plan)
    }

    pub fn run_worker(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
    ) -> Result<AttemptRun, UpstrokeError> {
        let identities = AttemptIdentities::new(site.key, site.generation, plan.attempt);
        let invocation = identities.worker();
        let request = worker_request(
            plan.worker.clone(),
            site.worktree.to_path_buf(),
            plan.agent.clone(),
            plan.worker_timeout,
            invocation.clone(),
        );
        let worker = self.execute(&request, plan.pool.clone())?;
        Ok(AttemptRun { identities, worker })
    }

    pub fn capture(&mut self, site: AttemptSite<'_>) -> Result<Capture, UpstrokeError> {
        self.manager
            .candidate_stage(self.hooks.effects(), site.slot)?;
        let tree = self
            .manager
            .candidate_write_tree(self.hooks.effects(), site.slot)?;
        Ok(Capture {
            tree,
            parent: site.base.0.clone(),
        })
    }

    pub fn assess(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        run: &AttemptRun,
        capture: &Capture,
        diff: &str,
        kind: crate::ir::TaskKind,
    ) -> Result<Assessment, UpstrokeError> {
        let adapter =
            self.adapters
                .get(plan.agent.as_str())
                .ok_or_else(|| UpstrokeError::Refused {
                    message: format!(
                        "this attempt ran as agent `{}` and no adapter answers to that name",
                        plan.agent.as_str()
                    ),
                })?;
        let mut outcome = adapter.parse(&run.worker)?;
        outcome.diff = diff.to_owned();

        let mut failure = crate::engine::attempt::evaluate_outcome(&outcome, &run.worker);
        if failure.is_none() {
            failure = crate::engine::classify::diff_failure(
                &outcome.diff,
                kind,
                !plan.reviewers.is_empty(),
            );
        }
        if failure.is_none() {
            if let Some(problem) = self.input_policy.problem(site.worktree, &capture.tree)? {
                failure = Some(crate::engine::classify::review_input_failure(problem));
            }
        }
        Ok(Assessment { outcome, failure })
    }

    pub fn judge(
        &mut self,
        site: AttemptSite<'_>,
        plan: &AttemptPlan,
        judging: Judging<'_>,
        inputs: &ReviewInputs,
        invocations: &dyn Fn(u32) -> review::ReviewInvocations,
    ) -> Result<Judgement, UpstrokeError> {
        let Judging {
            run,
            capture,
            assessed,
        } = judging;
        let generation = site.generation.0;
        let attempt = plan.attempt.0;

        let mut failure = assessed.failure.clone();
        let mut gates = Vec::with_capacity(plan.gates.len());
        if !plan.gates.is_empty() && failure.is_none() {
            let snapshot = self.snapshot(SnapshotName::gates(generation, attempt), capture)?;
            for (index, gate) in plan.gates.iter().enumerate() {
                let invocation = run
                    .identities
                    .gate(u32::try_from(index).unwrap_or(u32::MAX), 0);
                let request = gate_request(
                    gate.command.clone(),
                    snapshot.path().to_path_buf(),
                    gate.timeout,
                    invocation,
                );
                let verdict = self.verdict(&request, None)?;
                let refused = !verdict.passed();
                if refused && failure.is_none() {
                    failure = Some(crate::engine::classify::gate_failure(&GateFailure {
                        gate: gate.name.clone(),
                        summary: format!(
                            "exit {}{}{}",
                            verdict
                                .code
                                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
                            if verdict.timed_out {
                                " (timed out)"
                            } else {
                                ""
                            },
                            if verdict.output_limited {
                                " (output truncated)"
                            } else {
                                ""
                            }
                        ),
                        log_tail: crate::util::tail(
                            &verdict.log,
                            crate::gates::FEEDBACK_TAIL_BYTES,
                        ),
                    }));
                }
                gates.push(verdict);
                if refused {
                    break;
                }
            }
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        let mut reviews = Vec::with_capacity(plan.reviewers.len());
        for (index, reviewer) in plan.reviewers.iter().enumerate() {
            if failure.is_some() {
                break;
            }
            let pass = u32::try_from(index).unwrap_or(u32::MAX);
            let snapshot =
                self.snapshot(SnapshotName::review(generation, attempt, pass), capture)?;
            let adapter = self.adapters.get(reviewer.agent.as_str()).ok_or_else(|| {
                UpstrokeError::Refused {
                    message: format!(
                        "review pass {pass} is bound to agent `{}` and no adapter answers to that \
                         name; pre-flight probed the agents this run recorded and this is not one \
                         of them",
                        reviewer.agent.as_str()
                    ),
                }
            })?;
            let outcome = self.reviews.run(
                &review::ReviewCx {
                    adapter,
                    profile: reviewer.profile.clone(),
                    lens: reviewer.lens,
                    task: review::ReviewSubject {
                        title: &inputs.title,
                        body: &inputs.body,
                        acceptance: &inputs.acceptance,
                    },
                    diff: &inputs.diff,
                    artifacts: &inputs.artifacts,
                    decisions: &inputs.decisions,
                    workspace: snapshot.path(),
                    settings_dir: &self.paths.settings(),
                    reviews_dir: &self.paths.reviews(),
                    stem: format!("{}-{}", inputs.stem, attempt),
                    timeout: reviewer.timeout,
                },
                self.runner,
                &invocations(pass),
            )?;

            let ids = invocations(pass);
            for ordinal in 0..outcome.invocations {
                let id = if ordinal == 0 {
                    ids.pass.clone()
                } else {
                    run.identities.review_reask(pass, ordinal - 1)
                };
                self.ledger.register(&id)?;
                self.ledger.complete(&id)?;
            }

            let unavailable = matches!(outcome.result, review::ReviewResult::Unavailable { .. });
            let cost_usd = outcome.cost_usd;
            failure = review_failure(outcome.result);
            reviews.push(
                super::super::classify::ReviewPassFacts {
                    pass: reviewer.lens.name(),
                    agent: &reviewer.profile.agent,
                    model: &reviewer.profile.model,
                    adapter: adapter.id(),
                    preflight_cli_version: reviewer.preflight_cli_version.clone(),
                    effort: reviewer.profile.effort,
                    pool: crate::engine::attempt::pool_option(&reviewer.profile.pool),
                    cost_usd,
                    unavailable,
                    failed: failure.is_some(),
                }
                .record(),
            );
            self.manager
                .remove_snapshot(self.hooks.effects(), &snapshot)?;
        }

        Ok(Judgement {
            gates,
            reviews,
            failure,
        })
    }

    fn snapshot(
        &mut self,
        name: SnapshotName,
        capture: &Capture,
    ) -> Result<Snapshot, UpstrokeError> {
        self.manager.add_snapshot(
            self.hooks.effects(),
            &name,
            &SnapshotInput::Tree {
                tree: captured_object_id("`git write-tree`", capture.tree.clone())?,
                parent: captured_object_id("the recorded base commit", capture.parent.clone())?,
            },
        )
    }

    pub fn settle_interrupted(
        &mut self,
        dispatched: &Dispatched,
        attempt: AttemptNumber,
        outcome: AttemptOutcome,
    ) -> Result<(), UpstrokeError> {
        self.emit(TopologyEventBody::AttemptInterrupted {
            data: AttemptInterrupted4 {
                key: dispatched.key,
                generation: dispatched.generation,
                attempt,
                lease: dispatched.closing_disposition(),
                detail: outcome.detail().to_owned(),
            },
        })?;
        self.discard_residue(dispatched)
    }

    pub fn cancel_in_flight(
        &mut self,
        dispatched: &Dispatched,
        attempt: AttemptNumber,
    ) -> Result<usize, UpstrokeError> {
        let cancelled = self.ledger.cancel_all_running();
        if let Some(held) = self.slots.held().cloned() {
            self.slots.release(&held)?;
        }
        self.settle_interrupted(dispatched, attempt, AttemptOutcome::Cancelled)?;
        Ok(cancelled)
    }

    fn discard_residue(&mut self, dispatched: &Dispatched) -> Result<(), UpstrokeError> {
        for slot in self.manager.intents()? {
            if matches!(slot, Slot::Snapshot { .. }) {
                self.manager.remove_worktree(self.hooks.effects(), &slot)?;
                self.manager.remove_intent(self.hooks.effects(), &slot)?;
            }
        }
        dispatch::scrub(self.manager, self.hooks, &dispatched.slot)
    }

    fn execute(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<ProcessOutput, UpstrokeError> {
        self.ledger.register(&request.invocation)?;
        match self.run_registered(request, pool) {
            Ok(output) => {
                if output.is_ok() {
                    self.ledger.complete(&request.invocation)?;
                } else {
                    self.ledger.cancel(&request.invocation)?;
                }
                output
            }
            Err(error) => {
                drop(self.ledger.cancel(&request.invocation));
                Err(error)
            }
        }
    }

    fn run_registered(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<Result<ProcessOutput, UpstrokeError>, UpstrokeError> {
        let slotted = is_slotted(&request.invocation);
        if slotted {
            let agent = request
                .agent
                .as_ref()
                .ok_or_else(|| UpstrokeError::Refused {
                    message: format!(
                        "`{}` is a slotted invocation and its request names no agent; the pair it \
                         would take is `{{agent, pool?}}` and there is no agent to key it by",
                        request.invocation
                    ),
                })?;
            self.slots.acquire(
                &request.invocation,
                SlotPair {
                    agent: agent.as_str().to_owned(),
                    pool,
                },
            )?;
        }
        let output = self.runner.run(request);
        if slotted {
            self.slots.release(&request.invocation)?;
        }
        Ok(output)
    }

    fn verdict(
        &mut self,
        request: &RunnerRequest,
        pool: Option<String>,
    ) -> Result<Verdict, UpstrokeError> {
        let output = self.execute(request, pool)?;
        Ok(Verdict {
            output_limited: output.output_limited,
            timed_out: output.timed_out,
            log: format!("{}{}", output.stdout, output.stderr),
            invocation: request.invocation.clone(),
            workspace: request.workspace.clone(),
            code: output.code,
        })
    }
}

#[cfg(test)]
mod tests;
