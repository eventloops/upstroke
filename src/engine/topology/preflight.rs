//! The `RunnerPreflight`: the one observation of shell and CLI availability
//! inside the run's boundary.
//!
//! [`crate::runner::container::resolve::RunnerPreflight`] is a **seam with no
//! production implementation**, and its own module says why: the probes are
//! `crate::runner::host::run_shell_probe` over the run's Runner, and building
//! that runner "needs a run identity, a private root and a slot pair that
//! `TopologyRun` owns at PR7". This is that implementation.
//!
//! INV-23, which is the whole specification of this file:
//!
//! > the `RunnerPreflight` — one non-slotted shell probe (the recorded shell
//! > executing `exit 0`) and one slotted probe per recorded agent, each a
//! > registered invocation through the run's Runner — executes at P4 and at
//! > every resume's recovery step (c) and is the only observation of shell and
//! > CLI availability inside the boundary.
//!
//! # The asymmetry is the point
//!
//! The shell probe is **non-slotted** and the agent probes are **slotted**.
//! `permits.agent_pool_slots` excludes two invocations by name — "gate
//! invocations and the shell probe acquire no slot" — and
//! [`crate::engine::topology::is_slotted`] is the total function of the
//! identity that decides it. Nothing here carries a second flag that could
//! disagree with the identity: the registration reads `is_slotted` and takes a
//! pair exactly when it answers `true`, so a shell probe that acquired a slot
//! would need `is_slotted` to be wrong about `ProbeTarget::Shell`.
//!
//! # Why the runner is wrapped rather than the probes hand-built
//!
//! An [`crate::agent::AgentAdapter`]'s `probe` runs **one or more** processes —
//! Codex runs four — and each is already a `RunnerRequest` carrying its own
//! `InvocationId` from [`crate::agent::probe_request`]. R4 is "invocation
//! registration (**all** Runner processes incl. gates, agent probes, and the
//! shell probe)", so registering only the identities this module could name in
//! advance would leave three of Codex's four unaccounted and the ledger would
//! balance while being wrong.
//!
//! So the ledger sits **at the boundary**: [`Registering`] is a
//! [`crate::runner::Runner`] that registers, slots, runs and settles every
//! request that passes through it, whoever built it. That is also what makes
//! `R3`'s "released on complete or cancel" and `R4`'s "completed or cancelled
//! exactly once" true of a probe that *fails*, which is the case
//! `resume_refuses_by_preflight_probe_when_shell_or_cli_fails_before_any_recovery_event`
//! exercises.
//!
//! # `certify` takes `&self`
//!
//! The trait's signature is `fn certify(&self, policy: &RunnerPolicy)`, and an
//! implementation that allocates identities has to mutate a ledger behind it.
//! [`std::sync::Mutex`] rather than a `RefCell`: [`crate::runner::Runner`] is
//! `Send + Sync`, and [`Registering`] is a `Runner`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::agent::{AdapterSource, Caps, ProcessOutput};
use crate::error::UpstrokeError;
use crate::gates::ShellKind;
use crate::runner::container::resolve::RunnerPreflight;
use crate::runner::{Runner, RunnerRequest};
use crate::topology::events::RunnerPolicy;

use super::identity::{InvocationLedger, PreflightIdentities, SlotAssertion, SlotPair, is_slotted};

/// What one pre-flight established.
///
/// The agents are carried in the order they were probed, because
/// `run_started(4).probed_agents` and `run_resumed(4).probed_agents` are lists
/// and a resume's list is compared against the record's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    /// Every agent whose CLI answered, in probe order.
    pub agents: Vec<String>,
    /// What each agent's CLI reported about itself.
    pub caps: Vec<(String, Caps)>,
}

impl Probed {
    /// The agent names, which is what the durable record carries.
    #[must_use]
    pub fn agents(&self) -> &[String] {
        &self.agents
    }
}

/// The run's `RunnerPreflight`: a shell probe and one probe per recorded agent.
///
/// Holds no `&mut` anything: the [`RunnerPreflight`] trait's `certify` takes
/// `&self` because `rebuild_from_record` holds the runner and the record while
/// it calls it.
pub struct RunPreflight<'a> {
    runner: &'a dyn Runner,
    adapters: &'a dyn AdapterSource,
    shell: ShellKind,
    workspace: PathBuf,
    agents: Vec<String>,
    /// Every invocation this pre-flight registered, in one process-local
    /// ledger. R4 balances at process end.
    ledger: Mutex<InvocationLedger>,
    /// R3, asserted rather than brokered: one slotted invocation at a time.
    slots: Mutex<SlotAssertion>,
    /// What the last `certify` established, for the caller that has to record
    /// `probed_agents`.
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
    /// The pre-flight of a run whose recorded shell is `shell` and whose
    /// recorded agents are `agents`.
    ///
    /// `workspace` is where the shell probe runs. The agent probes take
    /// [`crate::agent::probe_workspace`] through
    /// [`crate::agent::probe_request`] — "a probe asks a CLI about itself and
    /// has no workspace of its own" — and that decision is the adapter
    /// module's, not this one's.
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

    /// What the last successful [`RunnerPreflight::certify`] established.
    #[must_use]
    pub fn probed(&self) -> Option<Probed> {
        self.probed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether every invocation this pre-flight registered was settled exactly
    /// once, and no slot pair is still held.
    ///
    /// `resource_accounting` R3/R4 at `NoRunFinished`: "released (process
    /// death; empty at restart)" — but a *refusal* is not a process death, and
    /// a pre-flight that refused while still holding a slot would leave the
    /// resume's own later invocations refused by its own assertion. This is
    /// what `resume_preflight_probe_containers_reclaimed_after_refusal`
    /// asserts alongside the containers.
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

    /// How many probe invocations settled completed, and how many cancelled.
    ///
    /// Two numbers because R3 has two rows. A refused spawn is a **cancel** —
    /// the process either never started or produced no outcome this run may act
    /// on — and a boundary that completed it would leave a balanced ledger
    /// saying the opposite of what happened.
    #[must_use]
    pub fn settlements(&self) -> (usize, usize) {
        let ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        (ledger.completed(), ledger.cancelled())
    }

    /// Every identity still registered as running. Empty once `certify`
    /// returns, on either path.
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

    /// The runner every probe of this pre-flight actually executes through:
    /// the run's Runner, with the registration wrapped around it.
    fn registering(&self) -> Registering<'_> {
        Registering {
            inner: self.runner,
            ledger: &self.ledger,
            slots: &self.slots,
        }
    }
}

impl RunnerPreflight for RunPreflight<'_> {
    /// The shell probe, then one probe per recorded agent, in that order.
    ///
    /// **The order is the contract's**, and it is not alphabetical or
    /// arbitrary: `recovery_order` (c) writes "the non-slotted shell probe
    /// (recorded shell, exit 0) **and** one slotted probe per recorded agent",
    /// and `runner` adds "probes execute through it **sequentially** at
    /// pre-flight". A shell that cannot run a command is the cheaper refusal
    /// and the one that explains every agent failure that would follow it, so
    /// it goes first and the agent probes are never reached when it fails.
    ///
    /// `policy` is not consulted here and that is deliberate: the runner this
    /// executes through was *built* from the record — `rebuild_from_record`
    /// verified the runtime, the recorded image id and the volumes before
    /// calling this — so re-reading the record to decide anything would be
    /// this module deciding a question the rebuild already answered. It is
    /// named in the refusal, which is what a caller needs it for.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] naming the shell or the agent whose CLI did
    /// not answer. Every invocation registered before the refusal is cancelled
    /// and every slot pair released, so the ledgers balance on both paths.
    fn certify(&self, policy: &RunnerPolicy) -> Result<(), UpstrokeError> {
        let registering = self.registering();
        // (1) The shell. Non-slotted, and `is_slotted` is what makes that true
        // rather than an argument here.
        let shell_id = PreflightIdentities::shell(0)?;
        crate::runner::host::run_shell_probe(
            &registering,
            self.shell,
            self.workspace.clone(),
            shell_id,
        )
        .map_err(|error| refused(policy, &format!("the recorded shell: {error}")))?;

        // (2) One probe per recorded agent, sequentially, in recorded order.
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

/// The refusal shape every pre-flight failure takes.
///
/// Names the image the probe ran inside, because "a recorded shell or agent CLI
/// that fails **inside the recorded image**" is a different fact from the same
/// CLI failing on the host, and an operator who cannot tell which one has
/// nothing to fix.
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

/// The run's Runner with R3 and R4 wrapped around every request.
///
/// One place, so that "each a registered invocation" is true of a process an
/// adapter built as much as of one this module built.
struct Registering<'a> {
    inner: &'a dyn Runner,
    ledger: &'a Mutex<InvocationLedger>,
    slots: &'a Mutex<SlotAssertion>,
}

impl Runner for Registering<'_> {
    /// Register, slot, run, settle — and settle on the failure path too.
    ///
    /// The settlement is not in a `Drop`: a `Drop` that swallowed a refusal
    /// would make a ledger that did not balance look like one that did, and
    /// `permits.protocol` asks for "registered/completed/cancelled **exactly
    /// once**". A run that returns `Err` is a **cancel**, not a complete: the
    /// process either never started or did not produce an outcome this run may
    /// act on, and `R3`'s two lifecycle rows are "released on cancel" and
    /// "released on complete or cancel" precisely so the two are told apart.
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        {
            let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
            ledger.register(&request.invocation)?;
        }
        if is_slotted(&request.invocation) {
            let pair = SlotPair {
                // The agent whose per-agent slot this is. A slotted invocation
                // without an agent binding cannot be accounted, and refusing is
                // the assertion rather than inventing a name for it.
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
                // The pool is the routing layer's and arrives with PR11's
                // broker; a per-agent pair with no pool is the sequential
                // substrate's assertion, which is what R3 is here.
                pool: None,
            };
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            if let Err(error) = slots.acquire(&request.invocation, pair) {
                drop(slots);
                let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
                let _ = ledger.cancel(&request.invocation);
                return Err(error);
            }
        }

        let outcome = self.inner.run(request);

        if is_slotted(&request.invocation) {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
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
