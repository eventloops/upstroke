//! Extended notes: `docs/internals/engine/topology/preflight.md`

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::agent::{AdapterSource, Caps, ProcessOutput};
use crate::error::UpstrokeError;
use crate::gates::ShellKind;
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::{Runner, RunnerRequest};
use crate::topology::events::RunnerPolicy;

use super::identity::{InvocationLedger, PreflightIdentities, SlotAssertion, SlotPair, is_slotted};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub agents: Vec<String>,
    pub caps: Vec<(String, Caps)>,
}

impl Probed {
    #[must_use]
    pub fn agents(&self) -> &[String] {
        &self.agents
    }
}

pub struct RunPreflight<'a> {
    runner: &'a dyn Runner,
    adapters: &'a dyn AdapterSource,
    shell: ShellKind,
    workspace: PathBuf,
    agents: Vec<String>,
    ledger: Mutex<InvocationLedger>,
    slots: Mutex<SlotAssertion>,
    probed: Mutex<Option<Probed>>,
}

impl std::fmt::Debug for RunPreflight<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunPreflight")
            .field("shell", &self.shell)
            .field("workspace", &self.workspace)
            .field("agents", &self.agents)
            .finish_non_exhaustive()
    }
}

impl<'a> RunPreflight<'a> {
    #[must_use]
    pub fn new(
        runner: &'a dyn Runner,
        adapters: &'a dyn AdapterSource,
        shell: ShellKind,
        workspace: &Path,
        agents: Vec<String>,
    ) -> Self {
        Self {
            runner,
            adapters,
            shell,
            workspace: workspace.to_path_buf(),
            agents,
            ledger: Mutex::new(InvocationLedger::new()),
            slots: Mutex::new(SlotAssertion::new()),
            probed: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn probed(&self) -> Option<Probed> {
        self.probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn ledgers_balance(&self) -> bool {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .balances()
            && self
                .slots
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .balances()
    }

    #[must_use]
    pub fn settlements(&self) -> (usize, usize) {
        let ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        (ledger.completed(), ledger.cancelled())
    }

    #[must_use]
    pub fn running(&self) -> Vec<String> {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn registering(&self) -> Registering<'_> {
        Registering {
            inner: self.runner,
            ledger: &self.ledger,
            slots: Some(&self.slots),
        }
    }
}

impl RunnerPreflight for RunPreflight<'_> {
    fn certify(&self, policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
        let registering = self.registering();
        let shell_id = PreflightIdentities::shell(0)?;
        crate::runner::host::run_shell_probe(
            &registering,
            self.shell,
            self.workspace.clone(),
            shell_id,
        )
        .map_err(|error| refused(policy, &format!("the recorded shell: {error}")))?;

        let mut caps = Vec::with_capacity(self.agents.len());
        for agent in &self.agents {
            let adapter = self.adapters.get(agent).ok_or_else(|| {
                refused(policy, &format!("no adapter is registered for `{agent}`"))
            })?;
            let probed = adapter
                .probe(&registering)
                .map_err(|error| refused(policy, &format!("the `{agent}` CLI: {error}")))?;
            caps.push((agent.clone(), probed));
        }

        let probed = Probed {
            agents: self.agents.clone(),
            caps,
        };
        *self.probed.lock().unwrap_or_else(PoisonError::into_inner) = Some(probed);
        Ok(())
    }
}

fn refused(policy: &RunnerPolicy, what: &str) -> UpstrokeError {
    let boundary = match policy.image.as_ref() {
        Some(image) => format!(
            "the recorded image `{}` (id `{}`)",
            image.reference, image.id
        ),
        None => "this host".to_owned(),
    };
    UpstrokeError::Refused {
        message: format!(
            "pre-flight: {what}. It was probed inside {boundary}, which is the only observation \
             of shell and CLI availability inside this run's boundary; nothing was spawned for \
             the run and no recovery event was appended, so the run is resumable."
        ),
    }
}

pub(super) struct Registering<'a> {
    pub(super) inner: &'a dyn Runner,
    pub(super) ledger: &'a Mutex<InvocationLedger>,
    pub(super) slots: Option<&'a Mutex<SlotAssertion>>,
}

impl Runner for Registering<'_> {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        {
            let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
            ledger.register(&request.invocation)?;
        }
        if is_slotted(&request.invocation) {
            let Some(slots) = self.slots else {
                let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = ledger.cancel(&request.invocation);
                return Err(UpstrokeError::Refused {
                    message: format!(
                        "`{}` is a slotted invocation and this boundary holds no slots; INV-23's \
                         non-slotted probe is the recorded shell alone",
                        request.invocation
                    ),
                });
            };
            let pair = SlotPair {
                agent: match request.agent.as_ref() {
                    Some(agent) => agent.to_string(),
                    None => {
                        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
                        let _ = ledger.cancel(&request.invocation);
                        return Err(UpstrokeError::Refused {
                            message: format!(
                                "`{}` is a slotted invocation with no agent binding; the pair it \
                                 would take is `{{agent, pool?}}` and there is no agent to name",
                                request.invocation
                            ),
                        });
                    }
                },
                pool: None,
            };
            let mut slots = slots.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) = slots.acquire(&request.invocation, pair) {
                drop(slots);
                let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = ledger.cancel(&request.invocation);
                return Err(error);
            }
        }

        let outcome = self.inner.run(request);

        if let (true, Some(slots)) = (is_slotted(&request.invocation), self.slots) {
            let mut slots = slots.lock().unwrap_or_else(PoisonError::into_inner);
            slots.release(&request.invocation)?;
        }
        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        match &outcome {
            Ok(_) => ledger.complete(&request.invocation)?,
            Err(_) => ledger.cancel(&request.invocation)?,
        }
        outcome
    }
}

#[cfg(test)]
mod tests;
