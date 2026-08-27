//! The `RunnerPreflight` implementation, on its own.
//!
//! `recover/tests.rs` drives this through the whole recovery order; these are
//! the claims about the pre-flight itself — what it probes, in which order,
//! which of its processes take a slot pair, and that both ledgers balance on
//! the refusal path as well as the successful one.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::*;
use crate::agent::AgentAdapter;
use crate::gates::ShellKind;
use crate::ir::Outcome;
use crate::runner::invocation::AttemptRole;
use crate::runner::{AgentId, CommandSpec, InvocationId, ProbeTarget};
use crate::topology::events::{
    AttemptNumber, GenerationId, ImageIdentity, RunnerContract, RunnerKind,
};
use crate::topology::registry::TaskKey;

const AGENT: &str = "claude-code";
const OTHER: &str = "copilot";

// ---------------------------------------------------------------------------
// Doubles
// ---------------------------------------------------------------------------

/// A runner that records every request and can fail one program.
#[derive(Debug, Default)]
struct Recording {
    seen: Mutex<Vec<RunnerRequest>>,
    failing: Option<String>,
    /// Refuse to run at all — the shape a spawn failure has, as distinct from
    /// a process that ran and exited non-zero.
    refuses: bool,
}

impl Recording {
    fn failing(program: &str) -> Self {
        Self {
            failing: Some(program.to_owned()),
            ..Self::default()
        }
    }

    fn refusing() -> Self {
        Self {
            refuses: true,
            ..Self::default()
        }
    }

    fn programs(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|request| request.command.program.clone())
            .collect()
    }

    fn requests(&self) -> Vec<RunnerRequest> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Runner for Recording {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        if self.refuses {
            return Err(UpstrokeError::Refused {
                message: "the boundary refused to start this process".to_owned(),
            });
        }
        Ok(ProcessOutput {
            code: Some(
                if self.failing.as_deref() == Some(request.command.program.as_str()) {
                    127
                } else {
                    0
                },
            ),
            stdout: "9.9.9".to_owned(),
            stderr: String::new(),
            duration: Duration::from_millis(1),
            timed_out: false,
            output_limited: false,
        })
    }
}

/// An adapter that runs one probe process and reports what it printed.
#[derive(Debug)]
struct Stub(&'static str);

impl AgentAdapter for Stub {
    fn id(&self) -> &'static str {
        self.0
    }

    fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError> {
        let request = crate::agent::probe_request(
            self.0,
            CommandSpec::new(self.0).arg("--version"),
            0,
            Duration::from_secs(10),
        )?;
        let output = runner.run(&request)?;
        if output.code != Some(0) {
            return Err(UpstrokeError::Agent {
                message: format!("`{} --version` exited {:?}", self.0, output.code),
            });
        }
        Ok(Caps {
            version: output.stdout.trim().to_owned(),
            json_output: true,
            session_resume: false,
            cost_reporting: false,
            read_only_mode: false,
            acp: false,
            model_list: false,
        })
    }

    fn build(&self, _run: &crate::agent::TaskRun) -> Result<CommandSpec, UpstrokeError> {
        Ok(CommandSpec::new(self.0))
    }

    fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
        Err(UpstrokeError::Agent {
            message: "the fixture adapter runs no attempt".to_owned(),
        })
    }
}

struct Registry;

impl AdapterSource for Registry {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        match id {
            AGENT => Some(&Stub(AGENT) as &dyn AgentAdapter),
            OTHER => Some(&Stub(OTHER) as &dyn AgentAdapter),
            _ => None,
        }
    }
}

fn container_policy() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Container,
        policy: RunnerContract::ContainerV1,
        image: Some(ImageIdentity {
            reference: "ghcr.io/example/runner:2".to_owned(),
            id: "sha256:abc".to_owned(),
            digest: None,
        }),
        credential_volumes: Some(Default::default()),
    }
}

fn host_policy() -> RunnerPolicy {
    RunnerPolicy {
        kind: RunnerKind::Host,
        policy: RunnerContract::HostV1,
        image: None,
        credential_volumes: None,
    }
}

fn preflight<'a>(
    runner: &'a dyn Runner,
    adapters: &'a Registry,
    agents: &[&str],
) -> RunPreflight<'a> {
    RunPreflight::new(
        runner,
        adapters,
        ShellKind::Bash,
        Path::new("."),
        agents.iter().map(|agent| (*agent).to_owned()).collect(),
    )
}

// ---------------------------------------------------------------------------
// What a pre-flight is
// ---------------------------------------------------------------------------

/// One non-slotted shell probe, then one slotted probe per recorded agent, in
/// the recorded order.
///
/// Every clause of INV-23's sentence is a separate assertion, because they fail
/// independently: the count, the order, the shell going first, and the slot
/// asymmetry. Two agents rather than one, so "in the recorded order" has
/// something to be wrong about.
#[test]
fn a_preflight_is_the_shell_probe_then_one_probe_per_recorded_agent() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[OTHER, AGENT]);

    preflight
        .certify(&container_policy())
        .expect("every probe answers");

    assert_eq!(
        runner.programs(),
        vec!["bash".to_owned(), OTHER.to_owned(), AGENT.to_owned()],
        "the shell probe is first and the agents follow in the order the run recorded them"
    );
    let slotted: Vec<bool> = runner
        .requests()
        .iter()
        .map(|request| is_slotted(&request.invocation))
        .collect();
    assert_eq!(
        slotted,
        vec![false, true, true],
        "`permits.agent_pool_slots` excludes the shell probe by name and slots every agent probe"
    );
    let probed = preflight
        .probed()
        .expect("a successful certify records what it probed");
    assert_eq!(probed.agents(), [OTHER.to_owned(), AGENT.to_owned()]);
    assert_eq!(
        preflight.settlements(),
        (3, 0),
        "three processes ran and three completed; nothing was cancelled"
    );
    assert_eq!(
        probed
            .caps
            .iter()
            .map(|(agent, caps)| (agent.as_str(), caps.version.as_str()))
            .collect::<Vec<_>>(),
        vec![(OTHER, "9.9.9"), (AGENT, "9.9.9")],
        "and what each CLI said about itself"
    );
    assert!(preflight.ledgers_balance());
    assert!(preflight.running().is_empty());
}

/// The shell probe carries the identity [`PreflightIdentities::shell`] mints,
/// and each agent probe the one [`PreflightIdentities::agent`] mints.
///
/// Asserted as an equality against those constructors rather than against a
/// literal: the identities are what a container name is derived from, and a
/// test that spelled them out by hand would agree with a wrong implementation
/// as easily as with a right one.
#[test]
fn every_probe_carries_the_identity_the_preflight_mints_for_it() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[AGENT]);
    preflight
        .certify(&host_policy())
        .expect("both probes answer");

    let identities: Vec<InvocationId> = runner
        .requests()
        .iter()
        .map(|request| request.invocation.clone())
        .collect();
    assert_eq!(
        identities,
        vec![
            PreflightIdentities::shell(0).expect("the shell identity"),
            PreflightIdentities::agent(AGENT, 0).expect("the agent identity"),
        ]
    );
}

/// A failing shell refuses **before any agent is probed**, and names the image
/// the probe ran inside.
///
/// The second half is not decoration: "a recorded shell or agent CLI that fails
/// **inside the recorded image**" is a different fact from the same CLI failing
/// on the host, and an operator told only the first has nothing to fix.
#[test]
fn a_failing_shell_refuses_before_any_agent_is_probed() {
    let runner = Recording::failing("bash");
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[AGENT]);

    let error = preflight
        .certify(&container_policy())
        .expect_err("a shell that cannot run `exit 0` refuses");
    let text = error.to_string();
    assert!(text.contains("the recorded shell"), "{text}");
    assert!(
        text.contains("ghcr.io/example/runner:2") && text.contains("sha256:abc"),
        "the refusal names the image and its id: {text}"
    );
    assert_eq!(
        runner.programs(),
        vec!["bash".to_owned()],
        "no agent is probed after the shell fails"
    );
    assert!(preflight.ledgers_balance(), "and the ledgers still balance");
    assert!(preflight.probed().is_none(), "nothing was certified");
}

/// A failing agent CLI refuses naming the agent, and every slot it took is
/// released.
///
/// The slot half is the one a `Drop`-based release would get wrong and a test
/// asserting only the message would miss: a pre-flight that refused while still
/// holding the pair would make the run's next slotted invocation refuse against
/// its own assertion.
#[test]
fn a_failing_agent_cli_refuses_naming_the_agent_and_releases_its_slot() {
    let runner = Recording::failing(AGENT);
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[AGENT]);

    let error = preflight
        .certify(&host_policy())
        .expect_err("a CLI that does not answer refuses");
    let text = error.to_string();
    assert!(text.contains(AGENT), "{text}");
    assert!(
        text.contains("this host"),
        "a host record names the host rather than an image: {text}"
    );
    assert!(
        preflight.ledgers_balance(),
        "R3 releases on cancel as well as on complete; still running: {:?}",
        preflight.running()
    );
}

/// A runner that refuses to start a process at all settles the invocation as
/// **cancelled**, not completed.
///
/// R3's two lifecycle rows are "released on cancel" and "released on complete
/// or cancel", so the two are told apart on purpose. A ledger that completed a
/// process which never ran would balance and be wrong.
#[test]
fn a_refused_spawn_cancels_its_registration_rather_than_completing_it() {
    let runner = Recording::refusing();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[AGENT]);

    preflight
        .certify(&host_policy())
        .expect_err("a boundary that will not spawn refuses");
    assert!(preflight.ledgers_balance());
    assert!(
        preflight.running().is_empty(),
        "the registration was settled: {:?}",
        preflight.running()
    );
    assert_eq!(
        preflight.settlements(),
        (0, 1),
        "settled as cancelled, not completed: nothing ran, so nothing completed"
    );
}

/// An agent the record names and no adapter provides refuses, naming it.
///
/// `run_started(4).probed_agents` is the allow-list every binding is drawn
/// from, so an agent in it that this build cannot probe is a run that cannot be
/// certified — not one that quietly certifies fewer agents than it recorded.
#[test]
fn an_agent_with_no_adapter_refuses_naming_it() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &["nobody-ships-this"]);

    let error = preflight
        .certify(&host_policy())
        .expect_err("an unknown agent refuses");
    assert!(error.to_string().contains("nobody-ships-this"), "{error}");
    assert!(preflight.ledgers_balance());
}

/// A slotted invocation with no agent binding is refused, and its registration
/// is cancelled rather than left running.
///
/// Reachable only through a hand-built request — [`crate::agent::probe_request`]
/// always sets `agent` — which is exactly why it is worth holding: the pair a
/// slotted invocation takes is `{agent, pool?}`, and there is no agent to name.
#[test]
fn a_slotted_invocation_without_an_agent_binding_is_refused() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[]);
    let registering = preflight.registering();

    let request = RunnerRequest {
        command: CommandSpec::new("claude"),
        workspace: PathBuf::from("."),
        role: crate::runner::ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new(AGENT))),
        timeout: Duration::from_secs(1),
        agent: None,
        invocation: PreflightIdentities::agent(AGENT, 0).expect("a slotted probe identity"),
    };
    let error = registering
        .run(&request)
        .expect_err("a slotted invocation with no agent binding cannot be accounted");
    assert!(error.to_string().contains("no agent binding"), "{error}");
    assert!(
        preflight.running().is_empty(),
        "the registration was cancelled: {:?}",
        preflight.running()
    );
    assert!(preflight.ledgers_balance());
    assert!(runner.programs().is_empty(), "and nothing was spawned");
}

/// Two invocations sharing one identity are refused: that is aliasing, not a
/// duplicate completion.
#[test]
fn one_identity_cannot_be_registered_twice() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[]);
    let registering = preflight.registering();

    let request = RunnerRequest {
        command: CommandSpec::new("bash"),
        workspace: PathBuf::from("."),
        role: crate::runner::ExecutionRole::Probe(ProbeTarget::Shell),
        timeout: Duration::from_secs(1),
        agent: None,
        invocation: PreflightIdentities::shell(0).expect("the shell identity"),
    };
    registering.run(&request).expect("the first runs");
    let error = registering
        .run(&request)
        .expect_err("the second shares an identity with the first");
    assert!(error.to_string().contains("already registered"), "{error}");
}

/// A gate identity acquires no slot pair, so the pre-flight's boundary does not
/// take one for it either.
///
/// The rule lives in [`is_slotted`], which is a total function of the identity;
/// this is the assertion that the boundary reads it rather than carrying a
/// second opinion.
#[test]
fn a_gate_invocation_through_the_boundary_takes_no_slot() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[]);
    let registering = preflight.registering();

    let gate = InvocationId::attempt(
        TaskKey(0),
        GenerationId(0),
        AttemptNumber(1),
        AttemptRole::Gate(0),
        0,
    );
    assert!(!is_slotted(&gate), "the rule itself");
    let request = RunnerRequest {
        command: CommandSpec::new("bash"),
        workspace: PathBuf::from("."),
        role: crate::runner::ExecutionRole::Gate,
        timeout: Duration::from_secs(1),
        // A gate carries no agent binding; a boundary that slotted it would
        // refuse here for want of one, which is what makes this assertion
        // sharp rather than incidental.
        agent: None,
        invocation: gate,
    };
    registering
        .run(&request)
        .expect("a gate needs no slot pair");
    assert!(preflight.ledgers_balance());
}

/// The `Debug` rendering names what the pre-flight is for without printing its
/// ledgers.
///
/// Asserted because the impl is hand-written: a derived one would print the
/// `Mutex`es, and a `RunPreflight` in a refusal message would then carry every
/// invocation identity the run has issued.
#[test]
fn the_debug_rendering_names_the_boundary_and_not_the_ledgers() {
    let runner = Recording::default();
    let adapters = Registry;
    let preflight = preflight(&runner, &adapters, &[AGENT]);
    let rendered = format!("{preflight:?}");
    assert!(rendered.contains("RunPreflight"), "{rendered}");
    assert!(rendered.contains("Bash"), "{rendered}");
    assert!(rendered.contains(AGENT), "{rendered}");
    assert!(
        !rendered.contains("Mutex"),
        "the ledgers are not part of the rendering: {rendered}"
    );
}
