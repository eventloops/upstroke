//! The Runner seam (DESIGN.md §8, §20; INV-18, INV-20, INV-22, INV-23).
//!
//! > **Runner** — Execute probes, workers, gates, and reviewers on the host or
//! > in a role-scoped container; owns cwd, mounts, environment, supervision,
//! > and timeout, never agent semantics or Git. (DESIGN.md:118)
//!
//! An adapter builds a data-only [`CommandSpec`]; a [`Runner`] decides where
//! it executes. That split is the whole point of the layer: "adapters never
//! learn about containers, and the runner learns nothing about agent semantics
//! beyond which per-agent credential volume to mount" (DESIGN.md:612).
//!
//! PR4 ships the host half. [`host::HostRunner`] implements the trait, resolves
//! the `host-v1` [`policy`] for the marker, the owner record and
//! `run_started(4).runner`, composes the base-plus-overlay environment, and
//! executes the `RunnerPreflight` shell probe. The container runner is PR6 and
//! an explicit non-goal here, as are the async surface and the slot broker.
//!
//! ## Why `run` is synchronous and still shaped like the async one
//!
//! `decisions.sequential_substrate.runner`: "Runner::run(&RunnerRequest) ->
//! ProcessOutput synchronous until PR11 (then a boxed Send future)".
//! DESIGN.md:250-256 says why the shape has to survive that change: every
//! async trait used behind `dyn` returns a boxed `Send` future, so the trait
//! must already be object-safe and its request must already be a single
//! borrowed value. It is, and [`Runner`] is `Send + Sync` so a `&dyn Runner`
//! can be held across the await points PR11 introduces.

pub mod container;
pub mod host;
pub mod invocation;
pub mod policy;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::agent::ProcessOutput;
use crate::agent::proc::SpawnHooks;
use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, HookHarness, HookPhase, Injection, InjectionMode, ProcessSite, SubEffectPoint,
};

pub use invocation::InvocationId;

/// What an adapter hands the runner (DESIGN.md:222).
///
/// ```text
/// struct CommandSpec { program: String, args: Vec<String>, env: Vec<(String, String)>, stdin: Vec<u8> } // env is an overlay
/// ```
///
/// Data only, and that is load-bearing rather than stylistic: it knows nothing
/// about where it will run, so the same value is executed by the host runner
/// and (PR6) by the container runner without an adapter ever learning which.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program to execute, as the **adapter** names it — and the runner
    /// resolves it against the environment the runner composes.
    ///
    /// A name, not a location. Until PR6 an adapter's `build` and `probe`
    /// located their CLI on the coordinator host's `PATH` and put the absolute
    /// host path here, which was invisible while the host runner was the only
    /// runner and its boundary *was* this machine. With a second boundary it is
    /// three failures — a CLI pinned in an image and absent on the host refused
    /// before the runtime is asked anything, every spec carrying a path that
    /// names nothing inside the image, and `Caps.version` certifying the host's
    /// CLI while the attempt runs the image's, which is DESIGN.md:612's
    /// sentence exactly. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` in
    /// `reviews/FINDINGS.md` is the entry; [`crate::agent::bin::Invocation::named`]
    /// is the repair.
    ///
    /// This is not a new shape for the field. [`crate::gates::ShellKind::spec`]
    /// has always put a bare `sh`, `bash`, `cmd` or `pwsh` here, for every gate
    /// and for the `RunnerPreflight` shell probe; the three agent CLIs were the
    /// exception. **A `String` was always wide enough** — DESIGN.md:222 freezes
    /// `program: String`, and a name fits where a path may not
    /// (`PR4-PROGRAM-PATH-NOT-UNICODE`).
    ///
    /// The corollary belongs to the *runner*: resolving a name is now a thing
    /// each boundary does, so each boundary owns which files a name may reach.
    /// `PR6D-HOST-RUNNER-RESOLVES-BY-PLATFORM-SEARCH` in `reviews/FINDINGS.md`
    /// records what the host runner's answer is today and who owns tightening
    /// it.
    pub program: String,
    pub args: Vec<String>,
    /// **An overlay**, not the environment. DESIGN.md:258: "`CommandSpec.env`
    /// overlays a runner-owned base rather than replacing it."
    pub env: Vec<(String, String)>,
    /// Bytes for the child's stdin. `Vec<u8>` rather than `String` because a
    /// prompt is text but a spec is a command, and the funnel writes bytes.
    pub stdin: Vec<u8>,
}

impl CommandSpec {
    /// A spec for `program` with no arguments, no overlay and no stdin.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    /// Append arguments.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add one overlay entry.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Set the stdin payload.
    #[must_use]
    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = stdin.into();
        self
    }
}

/// The agent a request is bound to.
///
/// Matches [`crate::agent::AgentAdapter::id`] — `claude-code`, `copilot`,
/// `codex` — because that is the identity the credential location, the slot
/// pair and the catalog are all keyed by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(String);

impl AgentId {
    /// The adapter id as its own type.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as recorded.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a probe certifies.
///
/// The contract's `invariants_introduced[1]`: "the probe role carries target
/// `Agent(name) | Shell`". Two targets rather than one flag, because the two
/// are accounted differently and the difference is an invariant, not a
/// detail: INV-18 has "every agent CLI invocation **incl. agent probes**
/// acquires its atomic {agent, pool?} pair while gates **and the shell probe**
/// register without slots".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProbeTarget {
    /// One recorded agent's CLI. Slotted.
    Agent(AgentId),
    /// The recorded shell executing `exit 0`. Non-slotted.
    Shell,
}

/// Which seat a process occupies (DESIGN.md:224), with the probe target the
/// contract adds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionRole {
    Probe(ProbeTarget),
    Implement,
    Gate,
    Review,
}

impl ExecutionRole {
    /// Every role, with both probe targets.
    ///
    /// Written out rather than derived so a role added later has to be added
    /// here too, and so every grid over roles covers both probe targets — the
    /// pair whose accounting differs.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::Probe(ProbeTarget::Shell),
            Self::Probe(ProbeTarget::Agent(AgentId::new(
                crate::agent::claude::ADAPTER_ID,
            ))),
            Self::Implement,
            Self::Gate,
            Self::Review,
        ]
    }

    /// Whether this role's process takes an atomic `{agent, pool?}` slot pair.
    ///
    /// R3: "agent slot + pool slot pair (worker, review, re-ask, agent probe)
    /// … the shell probe and gates are non-slotted". PR4 records the property;
    /// the broker that acts on it is PR11.
    #[must_use]
    pub fn is_slotted(&self) -> bool {
        match self {
            Self::Probe(ProbeTarget::Agent(_)) | Self::Implement | Self::Review => true,
            Self::Probe(ProbeTarget::Shell) | Self::Gate => false,
        }
    }

    /// The role as it is written in a record: `probe(shell)`, `probe(<agent>)`,
    /// `implement`, `gate`, `review`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Probe(ProbeTarget::Shell) => "probe(shell)".to_owned(),
            Self::Probe(ProbeTarget::Agent(agent)) => format!("probe({agent})"),
            Self::Implement => "implement".to_owned(),
            Self::Gate => "gate".to_owned(),
            Self::Review => "review".to_owned(),
        }
    }
}

impl std::fmt::Display for ExecutionRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// One process the runner is asked to execute (DESIGN.md:223 plus the
/// contract's `invocation` field).
#[derive(Debug, Clone)]
pub struct RunnerRequest {
    pub command: CommandSpec,
    /// The child's working directory.
    pub workspace: PathBuf,
    pub role: ExecutionRole,
    pub timeout: Duration,
    /// The agent whose slot pair and credential location this process uses.
    /// `None` for a gate and for the shell probe.
    pub agent: Option<AgentId>,
    /// R4: "invocation registration (all Runner processes incl. gates, agent
    /// probes, and the shell probe)". Not optional — that is the invariant.
    pub invocation: InvocationId,
}

/// The worker process of one attempt: `ExecutionRole::Implement`, bound to the
/// agent whose CLI it is, carrying that attempt's worker identity.
///
/// One construction point per role, for the same reason
/// [`crate::agent::probe_request`] and [`host::shell_probe_request`] are one
/// each. The three fields below do not vary independently in production — the
/// role decides the slot pair (R3, [`ExecutionRole::is_slotted`]), the agent
/// binding decides which credential location `host-v1` supplies
/// ([`host::HostEnvironment::compose`]), and the identity form is the one
/// `decisions.admission_and_leases.permits.invocation_identity` gives a worker
/// — so a request that carried one without the others would be a request this
/// crate never sends. Before these existed, `HostRunner`'s own role grid
/// hand-built the worker and reviewer requests with `agent: None` and a *gate*
/// identity, which left a `HostRunner::run` that suppressed the containment
/// hooks for exactly the production shape (`role in {Implement, Review}` **and**
/// `agent.is_some()`) passing every test in the suite.
#[must_use]
pub fn worker_request(
    command: CommandSpec,
    workspace: PathBuf,
    agent: AgentId,
    timeout: Duration,
    invocation: InvocationId,
) -> RunnerRequest {
    RunnerRequest {
        command,
        workspace,
        // "`ExecutionRole::Implement` with the bound agent is what makes this
        // process slotted (R3) and what tells `host-v1` to supply that agent's
        // credential location — both properties of the role, not of the call
        // site."
        role: ExecutionRole::Implement,
        timeout,
        agent: Some(agent),
        invocation,
    }
}

/// One reviewer process: `ExecutionRole::Review`, bound to the reviewing
/// agent, carrying that pass's or re-ask's identity. See [`worker_request`].
///
/// A reviewer is an agent CLI, so it is slotted and `host-v1` gives it its
/// agent's credential location — the same rule as the worker, and the reason
/// the two share a shape rather than each being spelled out where it is sent.
#[must_use]
pub fn review_request(
    command: CommandSpec,
    workspace: PathBuf,
    agent: AgentId,
    timeout: Duration,
    invocation: InvocationId,
) -> RunnerRequest {
    RunnerRequest {
        command,
        workspace,
        role: ExecutionRole::Review,
        timeout,
        agent: Some(agent),
        invocation,
    }
}

/// One gate process: `ExecutionRole::Gate`, and **no** agent. See
/// [`worker_request`].
///
/// "A gate is repository-controlled code and runs no agent CLI, so it takes no
/// `{agent, pool}` pair (R3) and `host-v1` hands it no agent's credential
/// directory." `agent: None` is therefore part of what a gate *is*, not an
/// omission at the call site — which is why it is written once, here.
#[must_use]
pub fn gate_request(
    command: CommandSpec,
    workspace: PathBuf,
    timeout: Duration,
    invocation: InvocationId,
) -> RunnerRequest {
    RunnerRequest {
        command,
        workspace,
        role: ExecutionRole::Gate,
        timeout,
        agent: None,
        invocation,
    }
}

/// DESIGN.md:227.
///
/// Object-safe, and `Send + Sync` so PR11 can turn `run` into a boxed `Send`
/// future behind the same `&dyn Runner` its callers already hold.
pub trait Runner: Send + Sync {
    /// Execute `request` and return what the process did.
    ///
    /// # Errors
    ///
    /// A pre-flight refusal (a reserved environment key in the overlay, a
    /// failing shell probe) or a spawn/supervision failure. A non-zero exit is
    /// not an error: it is a [`ProcessOutput`].
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError>;
}

// ---------------------------------------------------------------------------
// ST-07 evidence: the containment sub-effect points
// ---------------------------------------------------------------------------

/// The site every containment sub-effect point belongs to.
pub const SPAWN_SITE: EffectSiteId = EffectSiteId::Process(ProcessSite::Spawn);

/// Wires the process funnel's [`SpawnHooks`] onto PR3's [`HookHarness`].
///
/// The funnel names a point; the harness is keyed by `(site, point, mode)`,
/// because a mode is executed when its fault *fired* rather than when a funnel
/// walked past the place it would have fired. So one funnel call consults the
/// harness once per mode the point declares, and the first non-`Proceed`
/// answer wins. A point with one mode is consulted once; `AmbientJobJoined`,
/// the only containment point the packet gives an error contract
/// (`containment_sub_effects`: "failure refuses the write command"), is
/// consulted for both.
#[derive(Debug, Clone, Default)]
pub struct HarnessHooks {
    harness: Arc<Mutex<HookHarness>>,
}

impl HarnessHooks {
    /// Observe through `harness`.
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self { harness }
    }

    /// The harness this observer records into.
    #[must_use]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }
}

impl SpawnHooks for HarnessHooks {
    fn point(&mut self, point: SubEffectPoint) -> Injection {
        let mut decision = Injection::Proceed;
        for mode in point.modes() {
            let answer = self.point_mode(point, *mode);
            if decision == Injection::Proceed {
                decision = answer;
            }
        }
        decision
    }

    /// One mode, at the coordinate that mode belongs at. A funnel that fires a
    /// point's two modes at two coordinates calls this twice, once each; the
    /// harness is keyed by `(site, point, mode)`, so each lands on its own key.
    fn point_mode(&mut self, point: SubEffectPoint, mode: InjectionMode) -> Injection {
        let mut harness = self
            .harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        harness.hook(SPAWN_SITE, HookPhase::Point { point, mode })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::invocation::{AttemptRole, SequenceRole};
    use crate::topology::events::{AttemptNumber, GenerationId, SequenceId};
    use crate::topology::registry::TaskKey;

    #[test]
    fn command_spec_carries_exactly_the_four_frozen_fields() {
        let spec = CommandSpec::new("claude")
            .arg("-p")
            .env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "8000")
            .stdin(b"prompt".to_vec());
        assert_eq!(spec.program, "claude");
        assert_eq!(spec.args, vec!["-p".to_owned()]);
        assert_eq!(
            spec.env,
            vec![(
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS".to_owned(),
                "8000".to_owned()
            )]
        );
        assert_eq!(spec.stdin, b"prompt".to_vec());
    }

    /// The slotting split is R3's sentence, transcribed here rather than
    /// computed from the function under test.
    #[test]
    fn slotting_follows_r3_not_the_predicate() {
        let expected: Vec<(ExecutionRole, bool)> = vec![
            (ExecutionRole::Probe(ProbeTarget::Shell), false),
            (
                ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new("claude-code"))),
                true,
            ),
            (ExecutionRole::Implement, true),
            (ExecutionRole::Gate, false),
            (ExecutionRole::Review, true),
        ];
        assert_eq!(
            expected.len(),
            ExecutionRole::all().len(),
            "every role in the grid, and no more"
        );
        for (role, slotted) in expected {
            assert_eq!(role.is_slotted(), slotted, "R3 for {role}");
        }
    }

    /// Each role builder produces its role, its binding and nothing else's,
    /// and carries the spec and the identity through untouched.
    ///
    /// The builders are what every fixture and every call site now asks for, so
    /// a builder that named the wrong role — or bound an agent to a gate —
    /// would be wrong everywhere at once and invisible in a grid keyed on the
    /// same builders. The expected values here are written from R3 and from
    /// each role's own sentence, not read back out of the builder.
    #[test]
    fn each_role_builder_binds_what_its_role_binds() {
        let spec = CommandSpec::new("prog")
            .arg("--go")
            .env("UPSTROKE_OVERLAY", "1")
            .stdin(b"payload".to_vec());
        let workspace = PathBuf::from("/tmp/ws");
        let agent = AgentId::new("claude-code");
        let timeout = Duration::from_secs(11);
        let worker_id = InvocationId::attempt(
            TaskKey(1),
            GenerationId(0),
            AttemptNumber(2),
            AttemptRole::Worker,
            0,
        );
        let review_id = InvocationId::attempt(
            TaskKey(1),
            GenerationId(0),
            AttemptNumber(2),
            AttemptRole::ReviewPass(0),
            0,
        );
        let gate_id = InvocationId::attempt(
            TaskKey(1),
            GenerationId(0),
            AttemptNumber(2),
            AttemptRole::Gate(3),
            0,
        );

        let built = vec![
            (
                worker_request(
                    spec.clone(),
                    workspace.clone(),
                    agent.clone(),
                    timeout,
                    worker_id.clone(),
                ),
                ExecutionRole::Implement,
                Some(agent.clone()),
                worker_id,
            ),
            (
                review_request(
                    spec.clone(),
                    workspace.clone(),
                    agent.clone(),
                    timeout,
                    review_id.clone(),
                ),
                ExecutionRole::Review,
                Some(agent),
                review_id,
            ),
            (
                gate_request(spec.clone(), workspace.clone(), timeout, gate_id.clone()),
                ExecutionRole::Gate,
                None,
                gate_id,
            ),
        ];
        assert_eq!(built.len(), 3, "the three in-attempt roles");

        for (request, role, agent, invocation) in &built {
            assert_eq!(&request.role, role);
            assert_eq!(&request.agent, agent, "{role}: the binding R3 gives it");
            assert_eq!(
                request.agent.is_some(),
                request.role.is_slotted(),
                "{role}: the binding and the slot pair are the same fact"
            );
            assert_eq!(&request.invocation, invocation, "{role}: the identity");
            // The spec is carried, not rebuilt: an overlay or a stdin payload
            // dropped here would be dropped for every caller at once.
            assert_eq!(request.command, spec, "{role}: the command spec");
            assert_eq!(request.workspace, workspace, "{role}");
            assert_eq!(request.timeout, timeout, "{role}");
        }
        // Three builders, three distinct roles, and two of the three bind.
        let roles: std::collections::BTreeSet<String> = built
            .iter()
            .map(|(request, _, _, _)| request.role.label())
            .collect();
        assert_eq!(roles.len(), 3);
        assert_eq!(
            built
                .iter()
                .filter(|(request, _, _, _)| request.agent.is_some())
                .count(),
            2
        );
    }

    #[test]
    fn role_labels_name_the_probe_target() {
        assert_eq!(
            ExecutionRole::Probe(ProbeTarget::Shell).label(),
            "probe(shell)"
        );
        assert_eq!(
            ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new("codex"))).label(),
            "probe(codex)"
        );
        assert_eq!(ExecutionRole::Implement.label(), "implement");
        assert_eq!(ExecutionRole::Gate.label(), "gate");
        assert_eq!(ExecutionRole::Review.label(), "review");
        // Two probe targets never render the same, or a record could not tell
        // a slotted probe from a non-slotted one.
        let labels: std::collections::BTreeSet<String> = ExecutionRole::all()
            .iter()
            .map(ExecutionRole::label)
            .collect();
        assert_eq!(labels.len(), ExecutionRole::all().len());
    }

    #[test]
    fn the_runner_trait_is_object_safe() {
        // PR11 turns `run` into a boxed Send future behind this same `dyn`.
        // A trait that stopped being object-safe would fail to compile here
        // rather than at the migration.
        fn takes_dyn(_: &dyn Runner) {}
        let runner = host::HostRunner::new();
        takes_dyn(&runner);
        let boxed: Box<dyn Runner> = Box::new(host::HostRunner::new());
        takes_dyn(boxed.as_ref());
    }

    /// Proof test: "InvocationId uniqueness within a run incl. agent and
    /// shell probes".
    ///
    /// Uniqueness is **structural**, not statistical. The identities of a run
    /// are the tuples the packet enumerates, and distinct tuples render
    /// distinctly (`invocation::tests::distinct_tuples_render_distinctly`
    /// crosses every field). So what this proves is the other half: that the
    /// set of identities a whole run's worth of Runner processes carries —
    /// INV-20's "worker, gate, review, re-ask, agent probe, shell probe" — is
    /// exactly one per process, with no expected value taken from a generator.
    #[test]
    fn invocation_ids_are_unique_within_a_run_incl_agent_and_shell_probes() {
        const TASKS: u32 = 7;
        const ATTEMPTS: u32 = 3;
        const GATES: u32 = 4;
        const SEQUENCES: u32 = 2;
        const AGENTS: [&str; 3] = ["claude-code", "copilot", "codex"];

        /// One run's Runner processes, in the order the run would produce
        /// them. A function rather than a literal because it is called twice:
        /// a run whose identities are not a function of the run is a run whose
        /// identities are not deterministic.
        fn run_requests() -> Vec<RunnerRequest> {
            let mut requests: Vec<RunnerRequest> = Vec::new();
            let mut push = |role: ExecutionRole, agent: Option<AgentId>, invocation| {
                requests.push(RunnerRequest {
                    command: CommandSpec::new("prog"),
                    workspace: PathBuf::from("/tmp"),
                    role,
                    timeout: Duration::from_secs(1),
                    agent,
                    invocation,
                });
            };

            // Pre-flight (INV-23's RunnerPreflight): one non-slotted shell
            // probe and one slotted probe per recorded agent. The packet's
            // third form, "(probe, target: Agent(name) | Shell, ordinal)".
            push(
                ExecutionRole::Probe(ProbeTarget::Shell),
                None,
                InvocationId::probe(ProbeTarget::Shell, 0).expect("shell probe identity"),
            );
            for agent in AGENTS {
                let id = AgentId::new(agent);
                push(
                    ExecutionRole::Probe(ProbeTarget::Agent(id.clone())),
                    Some(id.clone()),
                    InvocationId::probe(ProbeTarget::Agent(id), 0).expect("agent probe identity"),
                );
            }
            // The run: every attempt of every task, its gates, and its review
            // pass — the packet's first form, "(key, generation, attempt,
            // role, ordinal)".
            for task in 0..TASKS {
                for attempt in 1..=ATTEMPTS {
                    let agent = AgentId::new(AGENTS[((task + attempt) % 3) as usize]);
                    let key = TaskKey(task);
                    let generation = GenerationId(0);
                    let attempt_no = AttemptNumber(attempt);
                    push(
                        ExecutionRole::Implement,
                        Some(agent.clone()),
                        InvocationId::attempt(key, generation, attempt_no, AttemptRole::Worker, 0),
                    );
                    for gate in 0..GATES {
                        push(
                            ExecutionRole::Gate,
                            None,
                            InvocationId::attempt(
                                key,
                                generation,
                                attempt_no,
                                AttemptRole::Gate(gate),
                                0,
                            ),
                        );
                    }
                    push(
                        ExecutionRole::Review,
                        Some(agent),
                        InvocationId::attempt(
                            key,
                            generation,
                            attempt_no,
                            AttemptRole::ReviewPass(0),
                            0,
                        ),
                    );
                }
            }
            // Integration transactions — the packet's second form,
            // "(sequence, role, ordinal)", whose roles exclude worker.
            for sequence in 0..SEQUENCES {
                push(
                    ExecutionRole::Gate,
                    None,
                    InvocationId::sequence(SequenceId(sequence), SequenceRole::Gate(0), 0),
                );
                push(
                    ExecutionRole::Review,
                    // A reviewer is an agent CLI in this form too, so it
                    // carries its agent. A grid whose sequence reviews bound
                    // no agent would be varying the role and the binding
                    // together and calling it one field.
                    Some(AgentId::new(AGENTS[(sequence % 3) as usize])),
                    InvocationId::sequence(SequenceId(sequence), SequenceRole::ReviewPass(0), 0),
                );
            }
            requests
        }

        let requests = run_requests();
        // The size comes from the run's shape, written here, not from the
        // vector under test.
        let expected = 1
            + AGENTS.len()
            + (TASKS * ATTEMPTS * (1 + GATES + 1)) as usize
            + (SEQUENCES * 2) as usize;
        assert_eq!(requests.len(), expected, "the grid is the size it claims");
        assert_eq!(expected, 134, "a run's worth of processes, not a handful");

        let ids: std::collections::BTreeSet<String> = requests
            .iter()
            .map(|request| request.invocation.render())
            .collect();
        assert_eq!(
            ids.len(),
            requests.len(),
            "two Runner processes of one run share an InvocationId"
        );
        // All three forms are in the set, and each is in it the number of times
        // the run's shape says.
        let counted = |prefix: &str| ids.iter().filter(|id| id.starts_with(prefix)).count();
        assert_eq!(counted("p."), 1 + AGENTS.len(), "the pre-flight probes");
        assert_eq!(
            counted("k"),
            (TASKS * ATTEMPTS * (1 + GATES + 1)) as usize,
            "the attempt form"
        );
        assert_eq!(counted("s"), (SEQUENCES * 2) as usize, "the sequence form");

        // The binding is R3's rule in every form, and it is a count rather
        // than a claim: a grid that let the agent binding ride along with the
        // role would prove the identities of a run this crate never executes.
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.agent.is_some() != request.role.is_slotted())
                .count(),
            0,
            "a request bound an agent to a non-slotted role, or left a slotted one unbound"
        );
        let bound = requests
            .iter()
            .filter(|request| request.agent.is_some())
            .count();
        assert_eq!(
            bound,
            AGENTS.len() + (TASKS * ATTEMPTS * 2) as usize + SEQUENCES as usize,
            "the agent probes, every worker and reviewer of every attempt, and the sequence \
             reviews — counted"
        );
        assert_eq!(
            requests.len() - bound,
            1 + (TASKS * ATTEMPTS * GATES) as usize + SEQUENCES as usize,
            "the shell probe and every gate — counted"
        );

        // Deterministic in the sequential substrate: the same run yields the
        // same identities. A generator that mints a fresh value per call — a
        // ULID, a counter, a clock — fails here, and this is the assertion
        // `crash_reconstruction` rests on when it builds a container name
        // "so deterministic InvocationIds never collide across incarnations
        // and no earlier ownership evidence is overwritten".
        let again: Vec<String> = run_requests()
            .iter()
            .map(|request| request.invocation.render())
            .collect();
        let first: Vec<String> = requests
            .iter()
            .map(|request| request.invocation.render())
            .collect();
        assert_eq!(
            first, again,
            "the run's identities are not a function of the run"
        );

        // The probes are in the set, and they are accounted the way INV-18
        // accounts them: agent probes slotted, the shell probe not.
        let probes: Vec<&RunnerRequest> = requests
            .iter()
            .filter(|request| matches!(request.role, ExecutionRole::Probe(_)))
            .collect();
        assert_eq!(probes.len(), 1 + AGENTS.len());
        assert_eq!(
            probes.iter().filter(|p| p.role.is_slotted()).count(),
            AGENTS.len(),
            "agent probes are slotted"
        );
        assert_eq!(
            probes.iter().filter(|p| !p.role.is_slotted()).count(),
            1,
            "the shell probe is not"
        );
        assert_eq!(
            probes
                .iter()
                .filter(|p| p.invocation.probe_target().is_some())
                .count(),
            probes.len(),
            "every probe request carries a probe identity"
        );

        // "changes with every attempt": the same task, agent and role at two
        // attempts are two invocations, and they differ in the attempt field
        // rather than by chance.
        let workers: Vec<&InvocationId> = requests
            .iter()
            .filter(|request| request.role == ExecutionRole::Implement)
            .map(|request| &request.invocation)
            .collect();
        assert_eq!(workers.len(), (TASKS * ATTEMPTS) as usize);
        assert_eq!(
            workers
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            workers.len()
        );
        let first_task: Vec<String> = workers
            .iter()
            .filter(|id| matches!(id, InvocationId::Attempt { key, .. } if *key == TaskKey(0)))
            .map(|id| id.render())
            .collect();
        assert_eq!(
            first_task,
            vec![
                "k0.g0.a1.worker.o0".to_owned(),
                "k0.g0.a2.worker.o0".to_owned(),
                "k0.g0.a3.worker.o0".to_owned(),
            ],
            "a retry attempt has a new attempt number"
        );
    }

    /// Every place production code starts a process, named and counted.
    ///
    /// `decisions.pr_sequence[5].slice_contract.invariants_introduced[0]`:
    /// "**every** CLI and gate process executes through Runner", and
    /// `gating`: "process funnel sites recorded". Recorded as a table with
    /// counts rather than as prose, because the failure mode is a *new* spawn
    /// appearing somewhere with nobody deciding whether it should have been
    /// routed. A file that grows one fails here until it is classified.
    ///
    /// What is scanned: `Command::new`, `.spawn()` and `run_with_timeout` in
    /// the production region of every `src/**/*.rs`. The production region is
    /// the file with every `#[cfg(test)] mod … { … }` block removed by brace
    /// matching at the module's own indentation — sound because
    /// `cargo fmt --check` is a gate, so a module's closing brace is the first
    /// line at exactly that indentation. `src/engine/tests.rs` is a whole test
    /// module (`engine/mod.rs`: `#[cfg(test)] mod tests;`) and is excluded as
    /// one.
    ///
    /// The three rows are the only production process starts in the tree, and
    /// each is classified against the passage that puts it there.
    /// One row of the parity obligation: what a runner was asked to run, and
    /// what the adapter made of what came back.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct ParsedRow {
        pub(crate) name: &'static str,
        pub(crate) adapter: &'static str,
        pub(crate) status: crate::ir::OutcomeStatus,
        pub(crate) detail: Option<String>,
        pub(crate) session: Option<String>,
        pub(crate) cost_usd: Option<f64>,
    }

    /// The adapter-parsing half of `decisions.tests_acceptance.parity`, as a
    /// function of the boundary.
    ///
    /// > host and container runners produce identical **adapter parsing** and
    /// > environment composition
    ///
    /// PR6 calls this with its container runner and compares the returned
    /// table against the host's, which is what "dropped in beside it" means:
    /// the fixtures, the specs and the expectations live here once, and the
    /// only thing that varies between the two runs is the `&dyn Runner`.
    ///
    /// It is a real chain rather than a stub — spec → runner → `ProcessOutput`
    /// → `AgentAdapter::parse` — because the claim is about the *seam*: an
    /// adapter never learns which runner produced the output it reads, and
    /// nothing but a runner actually producing it proves that.
    ///
    /// The child is the recorded shell echoing an environment variable, so the
    /// fixtures need no agent CLI and no writable scratch file, and no payload
    /// byte ever reaches a command line. The payload and the exit code ride in
    /// on `CommandSpec.env` — the overlay, which is therefore load-bearing
    /// here rather than decorative, and which is itself half of what the
    /// parity clause is about.
    pub(crate) fn adapter_parse_parity(
        runner: &dyn Runner,
        workspace: &std::path::Path,
    ) -> Vec<ParsedRow> {
        struct Fixture {
            name: &'static str,
            adapter: &'static str,
            payload: &'static str,
            code: i32,
        }

        // Two adapters with two different answer shapes (a JSON envelope and
        // plain text) and both exit dispositions: a parity table whose rows
        // all parse the same way would prove two runners agree about nothing.
        const FIXTURES: &[Fixture] = &[
            Fixture {
                name: "json envelope, exit 0",
                adapter: crate::agent::claude::ADAPTER_ID,
                payload: r#"{"session_id":"s-parity","total_cost_usd":0.5,"result":"the work is done","subtype":"success"}"#,
                code: 0,
            },
            Fixture {
                name: "json envelope, non-zero exit",
                adapter: crate::agent::claude::ADAPTER_ID,
                payload: r#"{"session_id":"s-parity","total_cost_usd":0.5,"result":"the work is done","subtype":"success"}"#,
                code: 3,
            },
            Fixture {
                name: "plain text, exit 0",
                adapter: crate::agent::copilot::ADAPTER_ID,
                payload: "wrote the encoder",
                code: 0,
            },
        ];

        /// Echo `$UPSTROKE_PARITY_PAYLOAD` and exit `code`, in the recorded
        /// shell's own dialect. The payload is never in the command line, so
        /// nothing here depends on either shell's quoting.
        fn script(code: i32) -> String {
            if cfg!(windows) {
                format!("echo %UPSTROKE_PARITY_PAYLOAD%& exit {code}")
            } else {
                format!("printf '%s\\n' \"$UPSTROKE_PARITY_PAYLOAD\"; exit {code}")
            }
        }

        FIXTURES
            .iter()
            .enumerate()
            .map(|(index, fixture)| {
                let adapter = crate::agent::by_id(fixture.adapter).expect("a shipped adapter");
                let command = crate::gates::ShellKind::native()
                    .spec(&script(fixture.code))
                    .env("UPSTROKE_PARITY_PAYLOAD", fixture.payload);
                let output = runner
                    .run(&RunnerRequest {
                        command,
                        workspace: workspace.to_path_buf(),
                        role: ExecutionRole::Implement,
                        timeout: Duration::from_secs(60),
                        agent: Some(AgentId::new(fixture.adapter)),
                        invocation: InvocationId::attempt(
                            TaskKey(0),
                            GenerationId(0),
                            AttemptNumber(1),
                            AttemptRole::Worker,
                            u32::try_from(index).unwrap_or(u32::MAX),
                        ),
                    })
                    .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
                let outcome = adapter
                    .parse(&output)
                    .unwrap_or_else(|error| panic!("{}: parse: {error}", fixture.name));
                ParsedRow {
                    name: fixture.name,
                    adapter: fixture.adapter,
                    status: outcome.status,
                    detail: outcome.detail,
                    session: outcome.session_id,
                    cost_usd: outcome.cost_usd,
                }
            })
            .collect()
    }

    /// The host side of the parity table, pinned.
    ///
    /// The expected rows are written from what each adapter's `parse`
    /// documents — a JSON envelope with `is_error` absent and exit 0 is
    /// `Completed` carrying `result`, its session and its cost; the same
    /// envelope after a non-zero exit is an `AgentError`; and Copilot has no
    /// envelope at all, so it reports no session and no cost even on success.
    /// None of them is read back from `parse`.
    #[test]
    fn the_host_runners_adapter_parsing_table_is_the_one_pr6_must_match() {
        let workspace = std::env::temp_dir();
        let rows = adapter_parse_parity(&host::HostRunner::new(), &workspace);
        use crate::ir::OutcomeStatus;
        assert_eq!(
            rows,
            vec![
                ParsedRow {
                    name: "json envelope, exit 0",
                    adapter: "claude-code",
                    status: OutcomeStatus::Completed,
                    detail: Some("the work is done".to_owned()),
                    session: Some("s-parity".to_owned()),
                    cost_usd: Some(0.5),
                },
                ParsedRow {
                    name: "json envelope, non-zero exit",
                    adapter: "claude-code",
                    status: OutcomeStatus::AgentError,
                    // The failure path reports the agent's own text, and the
                    // envelope's session and cost survive it: spend already
                    // happened.
                    detail: Some("the work is done".to_owned()),
                    session: Some("s-parity".to_owned()),
                    cost_usd: Some(0.5),
                },
                ParsedRow {
                    name: "plain text, exit 0",
                    adapter: "copilot",
                    status: OutcomeStatus::Completed,
                    detail: Some("wrote the encoder".to_owned()),
                    session: None,
                    cost_usd: None,
                },
            ],
            "the host runner's adapter parsing moved"
        );
        // Hostility as counts: two adapters, two statuses, three distinct
        // details, and both "reports a session" dispositions.
        let mut statuses: Vec<String> =
            rows.iter().map(|row| format!("{:?}", row.status)).collect();
        statuses.sort();
        statuses.dedup();
        assert_eq!(statuses.len(), 2);
        let adapters: std::collections::BTreeSet<_> = rows.iter().map(|row| row.adapter).collect();
        assert_eq!(adapters.len(), 2);
        assert_eq!(rows.iter().filter(|row| row.session.is_some()).count(), 2);
        assert_eq!(rows.iter().filter(|row| row.cost_usd.is_some()).count(), 2);
    }

    /// Every spawn this slice performs is filed under **one** site, and that
    /// site declares one adjacent event, one fault row and one observable
    /// order.
    ///
    /// `decisions.effect_site_inventory.identity` says each group's "variants
    /// are its semantic contexts", and every variant carries "its adjacent
    /// durable event … [and] its fault-matrix row id". PR3's `Process` group
    /// has two variants — `Spawn` and `Terminate` — and `Spawn` is
    /// `After(AttemptStarted)` / `T-ATTEMPT`. PR4 routes five roles through
    /// it, and two of them do not run inside an attempt at all: the shell
    /// probe and the agent probes are `RunnerPreflight`, which
    /// `workspace_candidates.run_creation` orders at **P4**, before P6's
    /// `run_started`. A crash prefix at a probe spawn is therefore
    /// effect-before-`run_started` (T-RUNSTART on a fresh run, T-RESUME on a
    /// resume) while the site it is recorded under says event-before-effect in
    /// T-ATTEMPT.
    ///
    /// **The site's own variants are not this slice's to add.** The site enum,
    /// its adjacency and its fault row are `src/topology/effects.rs` — PR3's,
    /// frozen here — and a probe context would be a *new variant* of an
    /// inventory the packet enumerates. That half is deferred, with an owner,
    /// in `reviews/FINDINGS.md`. What this test contributes is that the
    /// mismatch is counted rather than silent: the two roles are named here,
    /// so ST-07 evidence over `Process.Spawn` cannot be read as covering the
    /// probe prefixes.
    ///
    /// **This count discharges nothing about the hooks themselves, and must
    /// not be read as if it did.** Counting that two roles fall outside the
    /// site's declared context proves the mismatch exists; it does not prove
    /// the containment hooks execute on those roles, and a `HostRunner::run`
    /// that passed `NoHooks` for `Probe(_)` would leave this test green. That
    /// obligation is PR4's — `scope`'s "**probes**, workers, gates, reviews go
    /// through the Runner" and `proof_tests[3]`'s "containment sub-effect hook
    /// tests (ST-07 subset)" — and it is discharged at runtime, for all five
    /// roles, by `host::tests::every_role_reaches_the_containment_points_of_this_platform`
    /// and `host::tests::a_fault_armed_at_any_containment_point_stops_any_role`.
    #[test]
    fn the_spawn_site_files_every_role_under_one_context_and_the_count_says_which() {
        use crate::topology::effects::{Adjacent, DurableEvent, FaultRow, ObservableOrder};

        // Transcribed from PR3's inventory, not read back from PR4.
        assert_eq!(SPAWN_SITE, EffectSiteId::Process(ProcessSite::Spawn));
        assert_eq!(
            SPAWN_SITE.adjacent(),
            Adjacent::After(DurableEvent::AttemptStarted)
        );
        assert_eq!(SPAWN_SITE.fault_row(), FaultRow::TAttempt);
        assert_eq!(
            SPAWN_SITE.observable_orders(),
            &[ObservableOrder::EventBeforeEffect],
            "one order, which is why `A3-REG-001`'s order-free key stays \
             equivalent for this site rather than becoming live debt here"
        );

        // Which roles this slice spawns, and whether each runs inside an
        // attempt — i.e. after the durable event the site is adjacent to.
        // Written from the packet's own ordering of a run's phases.
        let roles: Vec<(ExecutionRole, bool)> = vec![
            (ExecutionRole::Implement, true),
            (ExecutionRole::Gate, true),
            (ExecutionRole::Review, true),
            (ExecutionRole::Probe(ProbeTarget::Shell), false),
            (
                ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new("claude-code"))),
                false,
            ),
        ];
        assert_eq!(
            roles.len(),
            ExecutionRole::all().len(),
            "every role this slice routes is classified here"
        );
        let outside: Vec<String> = roles
            .iter()
            .filter(|(_, inside)| !*inside)
            .map(|(role, _)| role.label())
            .collect();
        assert_eq!(
            outside,
            vec!["probe(shell)".to_owned(), "probe(claude-code)".to_owned()],
            "the pre-flight roles, whose spawns precede `run_started` and are \
             nevertheless recorded under a site adjacent to `attempt_started`"
        );
        assert_eq!(
            outside.len(),
            2,
            "two of the five roles spawn outside the context this site names — \
             counted so the boundary cannot grow in silence"
        );
    }

    /// The eight containment coordinates, pinned as literals.
    ///
    /// `containment_sub_effects` writes them out — "Spawn.AmbientJobJoined …,
    /// Spawn.CreatedSuspended …, Spawn.PrivateJobAssigned, Spawn.Resumed …;
    /// Unix: Spawn.ReaperStarted …, Spawn.PreExecPgidAndRegister, Spawn.Exec,
    /// Spawn.Registered" — and every check the suite made on that vocabulary
    /// was derived from the enum it is meant to pin: the generated registry,
    /// the `Display` impl and the serde round trip all read `SubEffectPoint`,
    /// so renaming a variant *and* its `name()` arm together left all of them
    /// agreeing on the new spelling and the suite green. The literal
    /// `Spawn.CreatedSuspended` existed in this tree only inside doc comments.
    ///
    /// This is the project's own upheld line — a suite that "compares its own
    /// serialization only against itself" has not pinned anything — applied
    /// where the packet freezes the spelling in prose. The enum is PR3's and
    /// frozen; the assertion is PR4's, because PR4 is the slice that made these
    /// eight coordinates load-bearing.
    ///
    /// Two spellings, because there are two: the coordinate the packet writes
    /// (from `name()`) and the wire form the enum serialises to (from
    /// `rename_all = "snake_case"`). Naming the Rust variant in the same row is
    /// deliberate — a rename of the variant itself stops this table compiling,
    /// which is the same failure by a shorter route.
    #[test]
    fn the_containment_coordinates_are_pinned_against_written_literals() {
        use crate::topology::effects::ProcessSite;

        // (variant, the coordinate `containment_sub_effects` writes, wire form)
        const PINNED: &[(SubEffectPoint, &str, &str)] = &[
            (
                SubEffectPoint::AmbientJobJoined,
                "Spawn.AmbientJobJoined",
                "\"ambient_job_joined\"",
            ),
            (
                SubEffectPoint::CreatedSuspended,
                "Spawn.CreatedSuspended",
                "\"created_suspended\"",
            ),
            (
                SubEffectPoint::PrivateJobAssigned,
                "Spawn.PrivateJobAssigned",
                "\"private_job_assigned\"",
            ),
            (SubEffectPoint::Resumed, "Spawn.Resumed", "\"resumed\""),
            (
                SubEffectPoint::ReaperStarted,
                "Spawn.ReaperStarted",
                "\"reaper_started\"",
            ),
            (
                SubEffectPoint::PreExecPgidAndRegister,
                "Spawn.PreExecPgidAndRegister",
                "\"pre_exec_pgid_and_register\"",
            ),
            (SubEffectPoint::Exec, "Spawn.Exec", "\"exec\""),
            (
                SubEffectPoint::Registered,
                "Spawn.Registered",
                "\"registered\"",
            ),
        ];

        let declared: std::collections::BTreeSet<SubEffectPoint> =
            SPAWN_SITE.sub_effects().iter().copied().collect();
        let pinned: std::collections::BTreeSet<SubEffectPoint> =
            PINNED.iter().map(|(point, _, _)| *point).collect();
        assert_eq!(
            pinned, declared,
            "the site declares a containment point this table does not pin"
        );
        assert_eq!(PINNED.len(), 8);

        for (point, coordinate, wire) in PINNED {
            assert_eq!(
                format!("{}.{}", ProcessSite::Spawn.name(), point.name()),
                *coordinate,
                "the coordinate the packet writes moved"
            );
            assert_eq!(
                serde_json::to_string(point).expect("encode a containment point"),
                *wire,
                "the wire form of {coordinate} moved"
            );
            // And the written literal decodes back to this point: a rename that
            // kept the encoder and the decoder agreeing would otherwise be
            // invisible from this direction too.
            let decoded: SubEffectPoint =
                serde_json::from_str(wire).expect("decode the written literal");
            assert_eq!(decoded, *point, "{coordinate} no longer accepts {wire}");
        }
    }

    /// Assignments to `field` **through a receiver**, in every assignment form.
    ///
    /// `x.field = …` and **all ten** of Rust's compound assignment operators —
    /// `+= -= *= /= %= &= |= ^= <<= >>=`; never `x.field == …`, and never a
    /// longer field whose name starts with this one.
    ///
    /// Two measured fail-open holes, both in the same direction. The literal
    /// `".field ="` misses `+=` — the idiomatic increment, and therefore the
    /// form a second counting rule is most likely to arrive in (S5 round 4).
    /// The five-operator repair for that then missed `&= |= ^=` (not in its
    /// set) and `<<= >>=` (second byte not `=`), which S5 round 5 measured with
    /// `task.attempts_on_rung |= 1;` (`R5-SETTLE-001`). The enumeration is now
    /// the language's, so there is no sixth hole of this shape.
    fn receiver_writes(code: &str, field: &str) -> usize {
        let needle = format!(".{field}");
        code.match_indices(&needle)
            .filter(|(at, _)| {
                let rest = code[at + needle.len()..].trim_start();
                match rest.as_bytes() {
                    // `==` is a comparison, not a write.
                    [b'=', b'=', ..] => false,
                    [b'=', ..] => true,
                    // `<<=` and `>>=`, whose second byte is not `=`.
                    [b'<', b'<', b'=', ..] | [b'>', b'>', b'=', ..] => true,
                    // The seven single-character compound operators. Rust has
                    // ten in total and this arm used to name five: `&= |= ^=`
                    // fell through it and `<<= >>=` never reached it, so
                    // `task.attempts_on_rung |= 1;` — a bare assignment through
                    // a receiver, inside the domain all three doc sentences
                    // state — left the census green. `R5-SETTLE-001`, and it is
                    // `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` five operators over.
                    [op, b'=', ..] => {
                        matches!(op, b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^')
                    }
                    _ => false,
                }
            })
            .count()
    }

    /// `value` up to `terminator` at **nesting depth zero**, so a comma inside
    /// `format!("{a}-{}", b)` does not end the expression.
    ///
    /// The reason this exists: taking a field initializer's value as "everything
    /// up to the first comma" truncates every multi-argument expression before
    /// its arguments, so the site is skipped rather than judged. Measured by S5
    /// round 4 — a planted `stem: format!("{index:02}-{}", display_id)` was
    /// invisible to the census that exists to forbid exactly that.
    fn until_depth_zero(value: &str, terminator: u8) -> &str {
        let mut depth = 0_i32;
        for (at, ch) in value.char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' if depth == 0 => return &value[..at],
                ')' | ']' | '}' => depth -= 1,
                _ if depth == 0 && ch as u32 == u32::from(terminator) => return &value[..at],
                _ => {}
            }
        }
        value
    }

    /// Every place production builds a filename `stem`, and the expression it
    /// builds it from.
    ///
    /// **Both shapes**: the field initializer `stem: <value>,` and the binding
    /// `let stem = <value>;`. The census counted only the first, so
    /// `coordinator.rs:537` — the live legacy path, and the site the schema-4
    /// assembler was extracted from — was outside its domain entirely.
    fn stem_values(code: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        for (at, _) in code.match_indices("stem") {
            let before = &code[..at];
            if before
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            {
                continue;
            }
            let rest = &code[at + "stem".len()..];
            let head = before.trim_end();
            let head = head.strip_suffix("mut").map_or(head, str::trim_end);
            if head.ends_with("let") {
                let Some(eq) = rest.find('=') else { continue };
                out.push((at, until_depth_zero(&rest[eq + 1..], b';').to_owned()));
            } else if let Some(tail) = rest.strip_prefix(':') {
                if tail.starts_with(':') {
                    continue;
                }
                out.push((at, until_depth_zero(tail, b',').to_owned()));
            }
        }
        out
    }

    /// Every `src/**/*.rs`, as `(repo-relative path, production code)`, with
    /// whole-file test modules left out.
    ///
    /// The region is [`crate::effects::production_code`]: the whole file with
    /// comments and string literals blanked and every `#[cfg(test)]` item
    /// removed. Every census below counts over it, and each of the three
    /// properties is load-bearing:
    ///
    /// * **Blanked**, because a count over raw text counts prose.
    ///   `src/agent/proc.rs` names `run_with_timeout` eight times, five in code
    ///   and three in doc comments, and a real `run_with_timeout_unbounded`
    ///   bypassing `OUTPUT_LIMIT_BYTES` could be paid for by deleting two
    ///   sentences in the same file. Measured — it was.
    /// * **The whole file**, because the previous region dropped everything
    ///   between a `#[cfg(test)] mod tests;` declaration and the next line that
    ///   is exactly `}`. Thirteen files declare their tests that way, and a
    ///   `Command::new("git").arg("push")` appended after one was invisible
    ///   while the identical lines above it failed the census.
    /// * **Item-wise removal**, not truncation, for the same reason.
    fn production_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, into: &mut Vec<PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .expect("read src")
                .map(|entry| entry.expect("entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, into);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    into.push(path);
                }
            }
        }

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        walk(&root.join("src"), &mut files);
        assert!(files.len() > 20, "the walk found the tree: {}", files.len());
        // Through the shared resolver: this loop was written out here and in
        // `events::log::tests`, and a third census then wrote a *different*
        // rule — `file_stem == "tests"` — which covers fourteen of the
        // seventeen. `PR7-R5-ATT-001`.
        let test_modules = crate::effects::census_domain::whole_file_test_modules(&files, 13);
        // The control: a derivation that found nothing would silently count
        // every test file as production, which is the failure this replaces.
        assert!(
            test_modules.contains(&root.join("src").join("engine").join("tests.rs")),
            "the `#[cfg(test)] mod tests;` derivation found no engine test module: {test_modules:?}"
        );
        fn dense(text: &str) -> usize {
            text.as_bytes()
                .iter()
                .filter(|byte| !byte.is_ascii_whitespace())
                .count()
        }

        let mut raw_bytes = 0_usize;
        let sources: Vec<(String, String)> = files
            .into_iter()
            .filter_map(|path| {
                if test_modules.contains(&path) {
                    return None;
                }
                let relative = path
                    .strip_prefix(&root)
                    .expect("under the manifest")
                    .to_string_lossy()
                    .replace('\\', "/");
                let source = std::fs::read_to_string(&path).expect("read source");
                raw_bytes += dense(&source);
                Some((relative, crate::effects::production_code(&source)))
            })
            .collect();
        // The three controls every census below shares, all of
        // `PR4-CENSUS-COMMENT-ORACLE`'s class.
        //
        // The regions are not empty, **file by file**. The aggregate floor below
        // is the weaker half of this and cannot replace it: it stands at
        // 750,000 against an actual 926,043, so 176,043 non-whitespace bytes may
        // vanish before it notices, and the two largest files this walk keeps
        // hold 146,260 between them — inside that headroom. One file's region
        // collapsing to nothing is exactly what a `#[cfg(test)]` in a comment
        // used to do, and it is invisible to a sum.
        //
        // **Necessary, not sufficient.** A per-file floor sees a region that
        // collapses; it does not see one that is *replaced*.
        // `PR7-R2C-CHAR-LITERAL-DESYNC`'s refined form removes exactly the
        // forged lines and adds a probe of the same size, and was measured at
        // 8525 dense bytes both with the attack and without it. What closes that
        // is `effects::char_literal_end` and `configured_item_end` returning
        // `start` rather than the file's length — not this.
        for (relative, code) in &sources {
            assert!(
                dense(code) > 0,
                "{relative}'s region is empty, so it contributes nothing to any count below \
                 and every prohibition this census states is vacuous for that file"
            );
        }
        let region_bytes: usize = sources.iter().map(|(_, code)| dense(code)).sum();
        assert!(
            region_bytes > 750_000,
            "the {} regions hold {region_bytes} non-whitespace bytes between them, so every \
             count below is over almost nothing",
            sources.len()
        );
        // And the blanking removed something. A blanking that silently stopped
        // working would put every doc comment and string literal back into the
        // counts, which is how a real ninth `run_with_timeout` entry point was
        // paid for by deleting two sentences.
        assert!(
            region_bytes < raw_bytes,
            "the sources hold {raw_bytes} non-whitespace bytes and the regions hold \
             {region_bytes}; the blanking removed nothing, so the counts below are over prose"
        );
        sources
    }

    /// Every `RunnerRequest` production builds is built by the builder for its
    /// role, and there are five roles and five builders.
    ///
    /// `scope`: "probes, workers, gates, reviews go through the Runner", and
    /// each of those four words is a role whose request carries three fields
    /// that travel together — the role, the agent binding (R3's slot pair,
    /// `host-v1`'s credential location) and the identity form. A request
    /// assembled at a call site can get one of them wrong; a request assembled
    /// by the role's builder cannot, and a *test* that assembles its own is
    /// how PR4 came to prove containment for a shape production never sends.
    ///
    /// So the census is on the construction, not on the shape: one
    /// `role: ExecutionRole::` per builder in the production region of the
    /// tree, and no others anywhere. A new hand-built request — in production
    /// or in a fixture that copied one — shows up here as a row that has to be
    /// classified.
    ///
    /// **Two needles, because one of them can be dodged.** A literal written
    /// with field shorthand (`RunnerRequest { command, workspace, role, … }`)
    /// names no variant and would slip past the first needle entirely — the
    /// grid in this very file writes one that way. So the type's own name is
    /// counted beside it, and that count includes the declaration and the
    /// builders' return types, which is why the numbers are what they are.
    #[test]
    fn every_production_runner_request_is_built_by_its_roles_builder() {
        use std::collections::BTreeMap;

        /// (file, `role: ExecutionRole::`, `RunnerRequest {`, and what they are).
        const EXPECTED: &[(&str, usize, usize, &str)] = &[
            (
                "src/agent/mod.rs",
                1,
                1,
                "probe_request: the agent probe, slotted and bound to the \
                 adapter it certifies",
            ),
            (
                "src/runner/host.rs",
                1,
                2,
                "shell_probe_request: the RunnerPreflight shell probe, \
                 non-slotted and bound to no agent — its literal, and the \
                 return type above it",
            ),
            (
                "src/runner/mod.rs",
                3,
                7,
                "worker_request, review_request, gate_request: the three \
                 in-attempt roles, where the binding is R3's rule rather than \
                 the call site's — three literals, three return types, and the \
                 declaration of the type itself",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (relative, production) in production_sources() {
            let counts = (
                production.matches("role: ExecutionRole::").count(),
                production.matches("RunnerRequest {").count(),
            );
            if counts != (0, 0) {
                found.insert(relative, counts);
            }
        }

        let expected: BTreeMap<String, (usize, usize)> = EXPECTED
            .iter()
            .map(|(file, roles, mentions, _)| ((*file).to_owned(), (*roles, *mentions)))
            .collect();
        assert_eq!(
            found, expected,
            "a RunnerRequest is built somewhere that is not its role's builder"
        );
        assert_eq!(
            expected.values().map(|(roles, _)| roles).sum::<usize>(),
            ExecutionRole::all().len(),
            "five roles, five construction points, and this is the count"
        );
        // The four words of `scope`, and the file that would hold a fifth.
        for absent in [
            "src/gates.rs",
            "src/review.rs",
            "src/engine/attempt.rs",
            "src/engine/coordinator.rs",
            // Assembles a worker's *command* and must never assemble its
            // request: the command says what to run and the request says the
            // role, the boundary and the identity. One module doing both would
            // be a call site that could choose its own role, which is what
            // `ExecutionRole::is_slotted` and `host::supplies_credentials` are
            // derived from.
            "src/engine/assembly.rs",
        ] {
            assert!(
                !expected.contains_key(absent),
                "{absent} assembles a request instead of asking for one"
            );
        }
    }

    #[test]
    fn every_production_process_start_is_classified() {
        use std::collections::BTreeMap;

        /// (file, `Command::new`, `.spawn()`, `run_with_timeout`) and why.
        const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[
            (
                "src/agent/proc.rs",
                1,
                2,
                3,
                "the process funnel itself: two `command.spawn()` (Unix and \
                 Windows), and three `run_with_timeout*` mentions in \
                 production CODE: `run_with_timeout_at`, its delegation \
                 to `run_with_timeout_and_limit`, and that private entry's \
                 declaration. The former plain `run_with_timeout` entry and \
                 its delegation are now inside a `#[cfg(test)]` support \
                 module; production callers must provide both Process sites \
                 explicitly, so counting those two test-only mentions as \
                 production would preserve the writable-handle-era fixture \
                 rather than the repaired boundary. There is also one \
                 `/bin/ps` on macOS that asks the kernel whether a process \
                 group has settled: a kernel query inside the reaper, not a \
                 CLI or a gate. It was **eight** while this census counted \
                 unblanked text: three of the eight are doc comments, and a \
                 real `run_with_timeout_unbounded` bypassing \
                 `OUTPUT_LIMIT_BYTES` could be paid for by deleting two \
                 sentences in this file. Measured — it was",
            ),
            (
                "src/runner/host.rs",
                1,
                0,
                1,
                "the host runner: `build_command` turns one CommandSpec into \
                 one Command, and `run` hands it to the funnel. This is where \
                 every routed process converges",
            ),
            (
                "src/runner/container.rs",
                1,
                0,
                0,
                "the Container funnel's `docker` CLI: one `Command::new(` in \
                 `DockerCli::exec`, which every container operation converges \
                 on, and no `.spawn()` because each is an `.output()` the \
                 funnel waits on. Deliberately NOT routed through the Runner, \
                 and for the same reason as the two Git rows below — \
                 DESIGN.md:612's \"authoritative Git and the event log never \
                 do\" is about the things that BUILD the boundary rather than \
                 execute inside it, and asking a container runtime what it \
                 holds is one of them. A `docker inspect` that went through \
                 the Runner would have to run inside the container whose \
                 existence it is establishing",
            ),
            (
                "src/workspace.rs",
                14,
                1,
                0,
                "authoritative Git, deliberately NOT routed. DESIGN.md:612 — \
                 \"Workers, repository-controlled gates, and reviewers all \
                 cross the boundary; authoritative Git and the event log \
                 never do.\" A git call that started going through the Runner \
                 would be a defect in the other direction",
            ),
            (
                "src/workspace_manager.rs",
                2,
                0,
                0,
                "the same decision as src/workspace.rs, for the schema-4 \
                 primitives: authoritative Git, deliberately NOT routed \
                 (DESIGN.md:612). Two `Command::new(` — one hook-free builder \
                 every effectful funnel goes through, and one read-only \
                 inspection helper the residue classifier uses — and no \
                 `.spawn()`, because every one of them is a `.output()` the \
                 funnel waits on. `decisions.workspace_candidates.manager` puts \
                 worktrees, snapshots, refs and Git objects behind these \
                 funnels; nothing here is a CLI, a gate, or a reviewer",
            ),
        ];

        fn count(haystack: &str, needle: &str) -> usize {
            haystack.matches(needle).count()
        }

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut found: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for (relative, production) in production_sources() {
            let counts = (
                count(&production, "Command::new("),
                count(&production, ".spawn()"),
                count(&production, "run_with_timeout"),
            );
            if counts != (0, 0, 0) {
                found.insert(relative, counts);
            }
        }

        let expected: BTreeMap<String, (usize, usize, usize)> = EXPECTED
            .iter()
            .map(|(file, new, spawn, timeout, _)| ((*file).to_owned(), (*new, *spawn, *timeout)))
            .collect();
        assert_eq!(
            found, expected,
            "production process starts moved. Every row is a decision \
             (DESIGN.md:612): route it through the Runner, or say here why it \
             is one of the things that never crosses the boundary"
        );
        // The table names five files, and it is the *set* that is the claim:
        // adapters, gates, review and the engine appear nowhere in it, which
        // is what "every CLI and gate process executes through Runner" means
        // once the migration has happened. All five really do start a process.
        // PR6's `src/runner/container.rs` is the newest, and its row says why a
        // `docker` call is one of the things that never crosses the boundary
        // rather than one that was forgotten.
        //
        // It was six. The sixth was `src/effects.rs`, whose only
        // `Command::new(` is inside `DENIAL_FIXTURES` — a string constant whose
        // whole purpose is to be REFUSED, compiled against `clippy.toml` by
        // `effects::tests::every_declared_effect_denial_refuses_for_the_reason_
        // it_declares`. It was a row here only because this census counted
        // string literals. It counts code now, so a fixture is not a process
        // start and `src/effects.rs` is named below with the rest of the files
        // that start none.
        assert_eq!(expected.len(), 5);
        for name in [
            "src/effects.rs",
            "src/gates.rs",
            "src/review.rs",
            "src/engine/attempt.rs",
            "src/agent/claude.rs",
            "src/agent/copilot.rs",
            "src/agent/codex.rs",
            "src/agent/bin.rs",
            "src/capacity.rs",
            "src/connect.rs",
        ] {
            assert!(
                !expected.contains_key(name),
                "{name} starts no process of its own"
            );
        }

        // The other half of DESIGN.md:612's sentence. "Authoritative Git and
        // the event log never [cross the boundary]" names two things, and the
        // table above only sees one of them: `src/workspace.rs` is caught by a
        // declared `Command::new(` count it would have to lose, but `events.rs`
        // legitimately starts no process at all, so a Runner call *appearing*
        // there subtracts from nothing. An event append implemented by
        // spawning an append helper through the Runner — on every event,
        // replay included — passed the census above unchanged.
        //
        // So the event log is asserted by name and by the tokens that would
        // mean it had acquired a boundary: not just a spawn, but a runner, a
        // request, or a command spec, any of which is the log deciding where
        // its writes execute.
        for (file, why) in [
            (
                "src/events/mod.rs",
                "the event vocabulary and fold: DESIGN.md:612 puts the event log, \
                 with authoritative Git, among the things that never cross the \
                 boundary",
            ),
            (
                // PR5 moved the writer here. The claim follows the code: this
                // file is now the only one that writes the log, so it is the
                // one an append-by-subprocess would have to appear in.
                "src/events/log.rs",
                "the event log writer: DESIGN.md:612 puts it, with authoritative \
                 Git, among the things that never cross the boundary",
            ),
            (
                "src/topology/events.rs",
                "the event vocabulary: data, and it stays data",
            ),
        ] {
            let source = std::fs::read_to_string(root.join(file)).expect("read the event log");
            let code = crate::effects::production_code(&source);
            for token in [
                "Command::new(",
                ".spawn()",
                "run_with_timeout",
                "HostRunner",
                "dyn Runner",
                "RunnerRequest",
                "CommandSpec",
            ] {
                assert_eq!(
                    count(&code, token),
                    0,
                    "{file} names `{token}`, so it can start a process. {why}"
                );
            }
        }

        // And an adapter does not *choose* a boundary either. DESIGN.md:117:
        // an adapter turns a TaskRun into a data-only CommandSpec and "does
        // not decide where the process runs". Naming a concrete runner in
        // production is that decision, whether or not it also spawns — which
        // is the half a spawn-site count cannot see. `capacity` and `connect`
        // are the two commands that legitimately make their own host runner
        // because they drive no run and have none to borrow, so they are
        // named here rather than covered by silence.
        for adapter in [
            "src/agent/mod.rs",
            "src/agent/bin.rs",
            "src/agent/claude.rs",
            "src/agent/copilot.rs",
            "src/agent/codex.rs",
        ] {
            let source = std::fs::read_to_string(root.join(adapter)).expect("read adapter");
            // Code, not prose: a doc comment may name the host runner to
            // explain why something is the way it is, and several do. The
            // blanking is what removes them; a `//`-prefixed-line filter left a
            // trailing `// … HostRunner …` on a code line in place.
            let code = crate::effects::production_code(&source);
            assert_eq!(
                count(&code, "HostRunner"),
                0,
                "{adapter} names a concrete boundary; an adapter receives one"
            );
        }
    }

    /// Write-command containment is joined in **one** place and proved in
    /// **one** place, and this is the count.
    ///
    /// `Contained`'s constructor is private to `runner::host`, so the only
    /// mutation that can mint a proof out of a failed join — `let _ =
    /// proc::join_ambient_job(hooks); Ok(Contained::new())` — is one that can
    /// be *written* inside that module, and nowhere else in the crate. That
    /// makes the class closable by counting: one call to
    /// `proc::join_ambient_job`, one call to `Contained::new`, both inside the
    /// function
    /// `host::tests::the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing`
    /// drives on its failure branch.
    ///
    /// A second mint appearing anywhere — a new entry point that "also"
    /// establishes containment, a facade that inlines the step — fails here
    /// until it is classified, which is the half a single failure-path test
    /// cannot cover on its own. Code only: several doc comments name both
    /// symbols, and two of them do it to explain this very rule.
    ///
    /// **Three needles, because the named constructor can be walked around.**
    /// `Contained`'s field is private to `runner::host`, and inside that module
    /// `Contained(())` builds one without going anywhere near `Contained::new`
    /// — and without touching the establishment counter the failure-path test
    /// reads. So the tuple-struct call is counted too, which is why
    /// `src/runner/host.rs` shows one (the declaration) and `src/main.rs` shows
    /// two.
    #[test]
    fn write_command_containment_has_one_join_site_and_one_mint() {
        use std::collections::BTreeMap;

        /// (file, `proc::join_ambient_job(`, `Contained::new()`, `Contained(`,
        /// and why).
        const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[
            (
                "src/main.rs",
                0,
                0,
                2,
                "a different type with the same name and shape: the CLI's own \
                 `containment::Contained`, which proves *classification* — a \
                 write command joined, a read-only one was not asked to. Its \
                 two are the declaration and `establish`'s own construction, \
                 and it joins through `runner::host::start_write_command` \
                 rather than calling `proc` itself",
            ),
            (
                "src/runner/host.rs",
                1,
                1,
                1,
                "contain_write_command: the step every public facade and \
                 `src/main.rs`'s dispatch reaches — one join, one mint. \
                 `HostRunner::start_write_command` calls it rather than \
                 repeating it, so a runner's own observer and production's \
                 `NoHooks` go through one body. The third count is the type \
                 declaration; a second `Contained(` here would be a mint that \
                 bypassed the counter",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for (relative, code) in production_sources() {
            // `production_sources` already blanks comments and string literals,
            // so a doc comment naming either symbol — and two of them do it to
            // explain this very rule — contributes nothing.
            let counts = (
                code.matches("proc::join_ambient_job(").count(),
                code.matches("Contained::new()").count(),
                code.matches("Contained(").count(),
            );
            if counts != (0, 0, 0) {
                found.insert(relative, counts);
            }
        }

        let expected: BTreeMap<String, (usize, usize, usize)> = EXPECTED
            .iter()
            .map(|(file, join, mint, built, _)| ((*file).to_owned(), (*join, *mint, *built)))
            .collect();
        assert_eq!(
            found, expected,
            "write-command containment is established somewhere new. Every row is a decision: \
             either it is the one step with the failure-path test behind it, or it is a second \
             one with none"
        );
    }

    /// Every production call site that **populates** a [`CommandSpec`] payload
    /// field, named and counted.
    ///
    /// This is the tripwire for `PR4-CONF-006`'s whole class. That finding was
    /// not "the fixtures forgot stdin"; it was that a production call site
    /// started filling a spec field and no fixture grid learned of it, so an
    /// observer suppression keyed on that field passed every test in the suite.
    /// The same thing is true of the overlay the moment anything sets one: as
    /// of this slice **nothing does**, and `runner::host::tests::
    /// the_role_grid_sends_the_shapes_production_sends` carries an empty
    /// overlay for all five roles *because* that is production's only value.
    ///
    /// So the count is on the population, not on the shape. `.stdin(` and
    /// `.env(` are counted across the production region of the tree — both
    /// `CommandSpec`'s builders and `std::process::Command`'s methods spell
    /// them the same way, and each row says which it is. A file that grows one
    /// fails here until somebody decides whether the grids have to carry it.
    ///
    /// **A method call is not the only way to populate a field.**
    /// `PR5-FIDELITY-001`: the two spec *constructors* build a `CommandSpec`
    /// with a struct literal, so `env: Vec::new()` at `src/agent/bin.rs`
    /// becoming an argument-dependent overlay is a production site this census
    /// could not see at all — pre-flight would then launch the probe with an
    /// overlay the spending command does not carry, against DESIGN.md:262-264.
    /// So the third column counts struct-literal `env:`/`stdin:` initializers
    /// too, and the constructors are enumerated rows like everything else.
    /// **A worker's command is assembled in exactly one production place, and
    /// a gate's in exactly one other.**
    ///
    /// The census this slice most needed and did not have. Two engines now want
    /// the same three command sets — the legacy one assembling them inline at
    /// the moment of use, the schema-4 driver wanting them up front in a plan —
    /// and assembling them twice is this project's dominant defect class. Of
    /// PR7's review findings the expensive ones were all one rule implemented
    /// twice, including two derivations of a task's predicted region that
    /// disagreed on every glob and shipped green (`84a3978`).
    ///
    /// **What this pins is input selection, not minting.** Minting was never
    /// duplicated: the crate has two `CommandSpec` constructors,
    /// `gates::ShellKind::spec` and `agent::bin::Invocation::spec`, and both
    /// already say so in their own docs. What was about to be duplicated is
    /// *which* prompt, permissions file, timeout and profile go into them. So
    /// the columns count the two calls that perform that selection —
    /// `AgentAdapter::build`, and `ShellKind::spec` — per file.
    ///
    /// `src/review.rs` is here with a non-zero `build` count **and that is the
    /// outstanding half of this work**, not an exemption: the reviewer's
    /// command is still assembled in `review.rs` because its invocation
    /// machinery is a re-ask loop with a per-invocation prompt, which does not
    /// extract by moving one expression. When that lands, its count moves the
    /// way the worker's did — and until then the duplication is a number in a
    /// test rather than a sentence in a review.
    #[test]
    fn a_command_is_assembled_in_one_production_place_per_role() {
        use std::collections::BTreeMap;

        /// (file, `AgentAdapter::build` calls, `ShellKind::spec` calls, why).
        const EXPECTED: &[(&str, usize, usize, &str)] = &[
            (
                "src/engine/assembly.rs",
                1,
                0,
                "the worker's command: the one production assembler. Both \
                 engines call it — the legacy one from `run_attempt`, the \
                 schema-4 driver when it builds an `AttemptPlan`",
            ),
            (
                "src/gates.rs",
                0,
                1,
                "the gate's command: `ShellGate::command`, the one production \
                 place a gate's cmdline becomes a spec. It lives on the type \
                 that owns the data rather than in the engine, because \
                 `gates.rs` sits below the engine",
            ),
            (
                "src/review.rs",
                1,
                0,
                "the reviewer's command. STILL ASSEMBLED HERE, and this row is \
                 the outstanding work rather than an exception: the re-ask \
                 loop builds a different prompt per invocation, so it does not \
                 move by lifting one expression",
            ),
            (
                "src/runner/host.rs",
                0,
                1,
                "the RunnerPreflight shell probe: a shell command that is not \
                 a gate. A different role, so a different assembler is correct \
                 — the count is here so that it is stated rather than assumed",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (path, code) in production_sources() {
            let builds =
                code.matches(".build(&task_run)").count() + code.matches(".build(&run)").count();
            let specs = code.matches(".spec(&self.cmd)").count()
                + code.matches(".spec(SHELL_PROBE_COMMAND)").count();
            if builds + specs > 0 {
                found.insert(path, (builds, specs));
            }
        }

        let expected: BTreeMap<String, (usize, usize)> = EXPECTED
            .iter()
            .map(|(path, builds, specs, _)| ((*path).to_owned(), (*builds, *specs)))
            .collect();
        assert_eq!(
            found, expected,
            "a production site selects the inputs for a command. Two sites for \
             one role is the duplication class that cost this slice its \
             predicted-region defect — classify it, and if it is a second \
             assembler for a role that already has one, it is the finding \
             rather than the fixture"
        );
    }

    /// **An attempt's ledger line is constructed in one production place.**
    ///
    /// The fourth one-production-place census, and the one whose subject is
    /// read back out. `AttemptRecord.failure` is what `ladder::next_step`
    /// decides from and what `ladder::spends_allowance` prices, so two
    /// constructions are two answers to "what happened to this attempt" — and
    /// the settlement, the escalation and the allowance would each be reading
    /// whichever one their caller happened to build.
    ///
    /// The column counts `AttemptRecord {` struct literals in production code,
    /// anchored to a line of its own for the reason the profile census is:
    /// a return type and the definition contain the same text.
    #[test]
    fn an_attempts_ledger_line_is_constructed_in_one_production_place() {
        use std::collections::BTreeMap;

        /// (file, `AttemptRecord {` literals, why).
        const EXPECTED: &[(&str, usize, &str)] = &[(
            "src/engine/classify.rs",
            1,
            "`attempt_record`. It was inline in `coordinator.rs`'s settlement, \
             out of the schema-4 driver's reach, and it lives beside the \
             failure classification rather than beside the command assembler \
             because its last field IS a classification",
        )];

        let mut found: BTreeMap<String, usize> = BTreeMap::new();
        for (path, code) in production_sources() {
            let records = code
                .lines()
                .filter(|line| line.trim() == "AttemptRecord {")
                .count();
            if records > 0 {
                found.insert(path, records);
            }
        }

        let expected: BTreeMap<String, usize> = EXPECTED
            .iter()
            .map(|(path, n, _)| ((*path).to_owned(), *n))
            .collect();
        assert_eq!(
            found, expected,
            "a production site decides what one attempt's durable line says. \
             Two sites is two answers to what happened, and the ladder reads \
             the answer back"
        );
    }

    /// **An invocation's profile is constructed in one production place per
    /// role.**
    ///
    /// The third of the one-production-place censuses, and the one that decides
    /// what a process is *allowed to do*. A [`crate::ir::WorkerProfile`] carries
    /// `permissions`, and the two roles want opposite answers: an implementer
    /// is `Edit`, a reviewer is `ReadOnly`. A driver that rebuilt an
    /// implementer's profile and reached for the nearest existing constructor
    /// would get `review::profile_for` — a read-only profile. The worker would
    /// spawn, edit nothing, and report success, and the gates would judge an
    /// empty diff.
    ///
    /// The column counts `WorkerProfile {` struct literals in production
    /// code, anchored to a line of its own: a return type (`-> WorkerProfile
    /// {`) and the definition itself (`struct WorkerProfile {`) both contain
    /// the same text and neither constructs anything. Measured without the
    /// anchor this census reported five sites and three files.
    #[test]
    fn a_worker_profile_is_constructed_in_one_production_place_per_role() {
        use std::collections::BTreeMap;

        /// (file, `WorkerProfile {` literals, why).
        const EXPECTED: &[(&str, usize, &str)] = &[
            (
                "src/engine/assembly.rs",
                1,
                "the implementer's profile, `permissions: Edit`. It was inline \
                 in `coordinator.rs`, out of the schema-4 driver's reach; both \
                 engines call it now",
            ),
            (
                "src/review.rs",
                1,
                "the reviewer's profile, `permissions: ReadOnly`. \
                 `PassBinding::profile` delegates here rather than repeating \
                 the literal, so the two roles are two sites and not four",
            ),
        ];

        let mut found: BTreeMap<String, usize> = BTreeMap::new();
        for (path, code) in production_sources() {
            let profiles = code
                .lines()
                .filter(|line| line.trim() == "WorkerProfile {")
                .count();
            if profiles > 0 {
                found.insert(path, profiles);
            }
        }

        let expected: BTreeMap<String, usize> = EXPECTED
            .iter()
            .map(|(path, n, _)| ((*path).to_owned(), *n))
            .collect();
        assert_eq!(
            found, expected,
            "a production site decides what a process may do. Two sites for \
             one role is how an implementer gets spawned read-only"
        );
    }

    /// **An observation about an attempt is classified in one production
    /// place.**
    ///
    /// The companion to `a_command_is_assembled_in_one_production_place_per_role`,
    /// and it pins the higher-stakes half. A command assembled twice runs the
    /// wrong process; a *classification* made twice decides the wrong thing
    /// about a task — `ladder::next_step` reads an `AttemptFailure` and chooses
    /// retry, escalate, defer, park or fail from it, and **the allowance
    /// decision is derived from the same field**. Two engines calling one diff
    /// different things would not surface as a wrong answer. It would surface
    /// as a task escalating to a pricier tier because the other engine
    /// disagreed about what its diff was.
    ///
    /// Columns count the constructors of the two verdict types: `AttemptFailure`
    /// and `ReviewRecord`. `src/engine/classify.rs` holds what was inline in
    /// the legacy verification ladder. `src/engine/attempt.rs` keeps its count
    /// for `review_failure`, which was **already a function** — a pure move of
    /// something already callable is churn, not extraction — and `src/ladder.rs`
    /// keeps its own for the escalation vocabulary it owns.
    #[test]
    fn an_observation_about_an_attempt_is_classified_in_one_production_place() {
        use std::collections::BTreeMap;

        /// (file, `AttemptFailure::new` calls, `ReviewPassOutcome::` uses, why).
        const EXPECTED: &[(&str, usize, usize, &str)] = &[
            (
                "src/capacity.rs",
                0,
                1,
                "reads an outcome to account for it. A reader, not a decider",
            ),
            (
                "src/engine/attempt.rs",
                8,
                0,
                "`review_failure`'s arms, the reviewer-unavailable mapping, \
                 and the outcome-status classification. Already functions and \
                 already reachable from the schema-4 driver, so they did not \
                 move — a pure move of something already callable is churn. \
                 `review_failure`'s own doc carries the rule the allowance \
                 derivation rests on: a reviewer asking for a human `must not \
                 spend an attempt or escalate`. Nine until the review-input \
                 refusal moved to `classify`, which is where the driver reads \
                 it from",
            ),
            (
                "src/engine/classify.rs",
                4,
                3,
                "what was INLINE in `run_attempt`'s verification ladder: the \
                 diff's two observations and a failed gate, plus the three \
                 arms that decide a review pass's outcome, plus the \
                 review-input refusal the ladder's third cheap rung raises. \
                 Both engines read these, and this is the only production site \
                 that CONSTRUCTS a `ReviewPassOutcome`",
            ),
            (
                "src/engine/coordinator.rs",
                0,
                1,
                "reads an outcome while reporting. A reader",
            ),
            (
                "src/export.rs",
                0,
                3,
                "reads outcomes to export them. A reader",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (path, code) in production_sources() {
            let failures = code.matches("AttemptFailure::new(").count();
            let records = code.matches("ReviewPassOutcome::").count();
            if failures + records > 0 {
                found.insert(path, (failures, records));
            }
        }

        let expected: BTreeMap<String, (usize, usize)> = EXPECTED
            .iter()
            .map(|(path, f, r, _)| ((*path).to_owned(), (*f, *r)))
            .collect();
        assert_eq!(
            found, expected,
            "a production site decides what an observation means about an \
             attempt. A second site for an observation that already has one is \
             two rules deciding one task's fate, and `ladder::next_step` and \
             the allowance derivation both read the answer"
        );
    }

    /// **The rung's allowance is counted in one production place, and decided
    /// in another that everyone consults.**
    ///
    /// The fourth single-authority census, and it exists because the pair it
    /// covers had already diverged. S5 round 2 found `TaskFold::attempts_on_rung`
    /// incrementing on every `attempt_started` while `ladder::spends_allowance`
    /// — documented as *"the single production implementation of the allowance
    /// rule"*, total over `FailureKind` — decided the same question at the
    /// settlement. Two production places for one rule, disagreeing on every
    /// interruption, park and outage, against
    /// `transaction_fault_matrix[T-ATTEMPT]`'s "unknown spend, **allowance
    /// refunded**".
    ///
    /// The other three censuses were each written after a defect of exactly this
    /// shape, and none of them covered *counting*: they cover which command a
    /// role gets, what an observation means, which profile a role runs under,
    /// and which ledger line an attempt writes. Repairing the divergence alone
    /// would leave the pair free to diverge again on the next edit, which is the
    /// difference between fixing an instance and closing a class.
    ///
    /// # The two columns
    ///
    /// **Writes** are **assignments through a receiver** — which is what makes a
    /// site a decider of persisted state. There may be exactly the two in the
    /// fold, the increment at the settlement and the reset the escalation
    /// performs onto its new rung, and no others anywhere. A third is a second
    /// counting rule.
    ///
    /// **Every assignment operator, not just `=`.** The needle was the literal
    /// `".attempts_on_rung ="`, which does not match `+=` — the most idiomatic
    /// form of the very thing this census counts. Measured by S5 round 4:
    /// planting `ladder.attempts_on_rung += 1;` in a production function left
    /// this census green. That is `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` one
    /// operator over: the stated domain ("assignments through a receiver")
    /// still exceeded the counted domain. [`receiver_writes`] counts the
    /// compound forms too, and excludes `==`.
    ///
    /// **The construction default is deliberately outside this domain, and the
    /// doc said otherwise until `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE`.**
    /// `TaskFold`'s `attempts_on_rung: 0` is a field initializer, not an
    /// assignment, so the needle never matched it while this comment claimed
    /// three sites and the table expected two. **A census whose stated domain is
    /// wider than its counted domain fails open**: a second `TaskFold`
    /// constructor with a non-zero default would move no count, and this
    /// census's whole purpose is that a new writer cannot appear silently.
    /// The domain is now stated as what it counts; widening it to constructors
    /// is a separate needle and a separate claim.
    ///
    /// **Consults** are calls to `spends_allowance`. These are *expected* to be
    /// plural: one rule consulted from several places is the shape this census
    /// wants. What it forbids is the alternative — a caller that re-derives the
    /// answer from a `SettlementTransition`, a `Next`, or an attempt number,
    /// which is what `settle_failed` did before `FailureShape` existed and what
    /// this fold did until round 2.
    #[test]
    fn the_rungs_allowance_is_counted_in_one_production_place() {
        use std::collections::BTreeMap;

        /// (file, `attempts_on_rung` writes, `spends_allowance` calls, why).
        const EXPECTED: &[(&str, usize, usize, &str)] = &[
            (
                "src/events/mod.rs",
                7,
                0,
                "**the legacy schema-3 progress tracker, and the reason this census exists rather than a bare repair.** It counts an attempt at its *start* and refunds by SUBTRACTION — five `saturating_sub` sites against one `saturating_add`, plus two resets. Each of those five is a place a future refund can be forgotten, which is the bug schema 4 shipped. **Recorded, not unified**: this is the legacy engine's own in-memory state, `invariants_preserved[1]` freezes its behaviour, and rewriting it would change the engine actually in production to tidy the one that is not. Zero consults is the finding in one number — the legacy engine never asks `spends_allowance`, because the rule was extracted FROM it",
            ),
            (
                "src/topology/fold.rs",
                2,
                1,
                "the only place schema 4 writes the count: one more at the settlement, and back to zero on the rung an escalation climbs onto. **No subtraction** — counting at the settlement makes T-ATTEMPT's refund the absence of a charge rather than a correction, which is the contrast with the seven sites above. The increment CONSULTS `spends_allowance` rather than answering for itself, and that is the whole of this census",
            ),
            (
                "src/engine/topology/run.rs",
                0,
                2,
                "TWO consults, both the same question about the one attempt the fold has not seen. The accepted-work arm returns `spends_allowance(None)` rather than a literal `true`; the failure arm adds this attempt to the rung before `next_step` runs, because `next_step` decides the transition the settlement will carry and the append has not happened yet. Consults, not a second rule, and no write",
            ),
            (
                "src/engine/topology/settle.rs",
                0,
                1,
                "`settle_failed` reads the allowance off the record rather than off the ladder's `Next` — the divergence `FailureShape`'s own doc records, on a park",
            ),
            (
                "src/ladder.rs",
                0,
                1,
                "the rule itself. `LadderState::attempts_on_rung` is a field this function reads, never one it writes",
            ),
        ];

        // **The control is a corpus entry, not a side call.**
        // `each_census_needle_covers_the_domain_its_doc_states` proves
        // `receiver_writes` is right and proves **nothing** about whether this
        // census calls it — measured: reverting the count below to the
        // pre-repair literal `code.matches(".attempts_on_rung =")` left that
        // unit test green, and left a closure-based self-check green too,
        // because the revert bypasses the closure. A control only binds the
        // census's own count if it travels through it, so this synthetic file
        // joins the corpus and is expected in the table like any other: it
        // carries **four** compound assignments — one from each shape the
        // needle has had to grow to cover, `+= -=` and `|= <<=` — and one
        // comparison, and expects 4. The pre-repair literal
        // `.attempts_on_rung =` scores it **1** (it matches only the `==`), and
        // the five-operator version scores it **2**. One compound assignment
        // and one comparison would have scored 1 under both the literal and the
        // correct needle, because the two errors cancel — measured, and the
        // reason the control is shaped this way.
        const SELF_CHECK: &str = "<self-check>";
        let mut corpus = production_sources();
        corpus.push((
            SELF_CHECK.to_owned(),
            "fn control() {\n    state.attempts_on_rung += 1;\n    state.attempts_on_rung -= 1;\n    \
             state.attempts_on_rung |= 1;\n    state.attempts_on_rung <<= 1;\n    \
             if state.attempts_on_rung == 1 {}\n}\n"
                .to_owned(),
        ));

        let mut found: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (path, code) in corpus {
            // **Assignments through a receiver**, which is what makes a site a
            // *decider* of persisted state. `let attempts_on_rung = ...` in the
            // driver is a local binding of a value it is about to pass, and
            // counting it would make this census report the consult twice.
            let writes = receiver_writes(&code, "attempts_on_rung");
            let consults = code.matches("spends_allowance(").count();
            if writes > 0 || consults > 0 {
                found.insert(path, (writes, consults));
            }
        }
        let expected: BTreeMap<String, (usize, usize)> = EXPECTED
            .iter()
            .map(|(path, w, c, _)| ((*path).to_owned(), (*w, *c)))
            .chain(std::iter::once((SELF_CHECK.to_owned(), (4, 0))))
            .collect();
        assert_eq!(
            found, expected,
            "one production place counts the rung's allowance and one decides \
             it. A second counting site is two rules deciding when an operator \
             pays for a pricier tier, and the first one diverged silently for \
             an entire slice"
        );

        // **And every settlement reaches that one place.** The table above
        // counts *write sites*, which is what it was written for — but a write
        // site nothing calls is a rule nothing applies, and that is exactly how
        // the allowance broke on 2026-08-27: `candidate_prepared` became the
        // sole successful settlement, the increment stayed behind in
        // `apply_settlement`, and this census went on finding its one write and
        // passing. A successful attempt spent nothing and nothing said so.
        //
        // Schema 4 has two settlement appliers — `apply_settlement` for a
        // failure and `apply_candidate_prepared` for a success — and both must
        // charge. Counting the calls is what makes a settlement that stops
        // charging a failing census rather than a silent undercount.
        let fold = std::fs::read_to_string("src/topology/fold.rs").expect("the fold reads");
        let production = crate::effects::production_code(&fold);
        let charges = crate::effects::census_domain::production_calls(
            &production,
            "self.charge_allowance",
            crate::effects::census_domain::Call::Free,
        );
        assert_eq!(
            charges, 2,
            "`charge_allowance` is called {charges} time(s) in the fold's production \
             region; schema 4 has two settlement appliers and each must charge the \
             rung — a failure through `apply_settlement` and a success through \
             `apply_candidate_prepared`"
        );
    }

    /// **Each census needle covers the domain its doc states.**
    ///
    /// The class boundary for three findings of one shape, all measured
    /// surviving in S5 round 4: a census whose **stated** domain is wider than
    /// its **counted** domain fails open, and does so in the passing direction,
    /// so nothing ever reports it. `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` was the
    /// first instance and was repaired by narrowing the prose; these are the
    /// same defect at three more needles, repaired by widening the needle,
    /// because in each case the wider domain is the one the census is for.
    ///
    /// Unit assertions over the needles themselves, deliberately: the censuses
    /// that use them are whole-tree and green, and a green whole-tree census is
    /// exactly what a fail-open needle produces.
    #[test]
    fn each_census_needle_covers_the_domain_its_doc_states() {
        // `receiver_writes`: every assignment form is a write, `==` is not, and
        // a longer field name is not this field.
        assert_eq!(receiver_writes("state.attempts = 1;", "attempts"), 1);
        assert_eq!(
            receiver_writes("state.attempts += 1;", "attempts"),
            1,
            "`+=` is the idiomatic increment and the form a second counting rule arrives in"
        );
        assert_eq!(receiver_writes("state.attempts -= 1;", "attempts"), 1);
        for compound in ["*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="] {
            assert_eq!(
                receiver_writes(&format!("state.attempts {compound} 1;"), "attempts"),
                1,
                "`{compound}` is one of Rust's ten compound assignment operators and this needle \
                 does not see it, so a second counting rule written that way is invisible"
            );
        }
        assert_eq!(receiver_writes("if state.attempts == 1 {}", "attempts"), 0);
        assert_eq!(
            receiver_writes("if state.attempts <= 1 {}", "attempts"),
            0,
            "a comparison is not a write, and `<=` is one byte from `<<=`"
        );
        assert_eq!(receiver_writes("if state.attempts >= 1 {}", "attempts"), 0);
        assert_eq!(receiver_writes("state.attempts_total = 1;", "attempts"), 0);
        assert_eq!(receiver_writes("let x = state.attempts;", "attempts"), 0);

        // `stem_values`: a field initializer's value runs to the end of the
        // statement rather than to the first comma...
        let field = stem_values("Inputs { stem: format!(\"{i:02}-{}\", display_id), n: 1 }");
        assert_eq!(field.len(), 1, "{field:?}");
        assert!(
            field[0].1.contains("display_id"),
            "the value was truncated at the comma inside `format!`, so the site is skipped \
             rather than judged: {:?}",
            field[0].1
        );

        // ...and a `let` binding is a stem site too, which is the shape the one
        // production site on the live legacy path uses.
        let binding =
            stem_values("let stem = format!(\"{i:02}-{}\", filename_component(task.id));\n");
        assert_eq!(binding.len(), 1, "{binding:?}");
        assert!(
            binding[0].1.contains("task.id") && binding[0].1.contains("filename_component"),
            "{:?}",
            binding[0].1
        );

        // A different identifier that merely ends in `stem` is not one.
        assert!(
            stem_values("let file_stem = path.file_stem();").is_empty(),
            "`file_stem` is not a filename stem built from a task id"
        );
    }

    /// **A plan-authored id never reaches a filename unsanitised.**
    ///
    /// The sixth single-authority census, and the one with the sharpest
    /// consequence. A task's `display_id` is whatever an `id=` annotation said —
    /// `plan/markdown.rs`'s `assemble` takes `Some(explicit) => explicit`
    /// verbatim, and `keys_by_display_id` checks only the reserved `repair-N-`
    /// prefix and duplicates. It then becomes a `stem`, and a stem becomes
    /// `dir.join(format!("{stem}.json"))` in every adapter's
    /// `materialize_permissions`.
    ///
    /// `PR7-R3-ATTEMPT-001`: the schema-4 assembler took `display_id` **raw**
    /// while `coordinator.rs:537`, the legacy authority it was extracted from,
    /// wrote `format!("{index:02}-{}", util::filename_component(task.id.as_str()))`.
    /// So `id=../../x` wrote outside the run directory. The extraction dropped a
    /// **guard**, which is the one-rule-two-places class at its worst — a
    /// dropped convenience is a bug, a dropped guard is a vulnerability.
    ///
    /// The census is on the **pairing**: wherever a `stem` is built from a
    /// plan-authored id, `filename_component` appears in the same expression.
    ///
    /// **It is also on a count, and the two say different things.** This
    /// paragraph used to end "that is what makes it survive a third assembler
    /// being written, which a count could not" — and the control below is now
    /// `assert_eq!(guarded + unguarded, 4)`, so a fourth *correctly guarded*
    /// assembler fails this test. That is deliberate (a new site has to be read
    /// once by a person, and a needle that quietly stops matching reads exactly
    /// like a clean tree) but it is the opposite of what the sentence promised.
    /// `R5-SETTLE-004`.
    ///
    /// **What the pairing cannot see, stated rather than left to be found**: a
    /// site that rebinds the id one line earlier — `let id = task.id.to_string();`
    /// and then a stem built from `id` — is not a site at all, so it never
    /// enters either list and the count still reads 4. `coordinator.rs` already
    /// carries a `let task_id = …` seven lines above the stem it builds, so the
    /// shape is not hypothetical. Reaching it needs data flow, not a needle.
    /// `R5-SETTLE-006`.
    ///
    /// `util::filename_component_neutralizes_hostile_names` is the other half —
    /// it asserts `"unit/fast"` becomes `"unit-fast"` and that an all-dots
    /// result becomes `"x"`, so `..` is neutralised. This census says the guard
    /// is *reached*; that test says it *works*.
    #[test]
    fn a_plan_authored_id_never_reaches_a_filename_unsanitised() {
        /// How this project spells a **plan-authored** task identifier. Both,
        /// because both engines are live: schema 4 froze the annotation onto
        /// `TaskEntry::display_id`, and the legacy coordinator — the path
        /// `upstroke run` still drives — reads `task.id`. The census used to
        /// name only the first, so the site the second one owns, which is the
        /// site the extraction was copied *from*, was outside its domain.
        const PLAN_AUTHORED: &[&str] = &["display_id", "task.id"];

        // **The control is a corpus entry**, for the reason the allowance census
        // above gives: a unit assertion on `stem_values` proves the helper and
        // says nothing about whether this census walks the tree with it. This
        // synthetic file is a guarded site of the exact shape the live legacy
        // path uses — a `let` binding whose value runs past a comma — so a
        // reader that matches only `stem:` initializers, or that truncates the
        // value at the first comma, loses it and the site count below is 3
        // rather than 4.
        const SELF_CHECK: &str = "<self-check>";
        let mut corpus = production_sources();
        corpus.push((
            SELF_CHECK.to_owned(),
            "fn control() {\n    let stem = format!(\"{i:02}-{}\", \
             filename_component(task.id));\n}\n"
                .to_owned(),
        ));

        let mut unguarded: Vec<String> = Vec::new();
        let mut guarded: Vec<String> = Vec::new();
        for (path, code) in corpus {
            for (at, value) in stem_values(&code) {
                if !PLAN_AUTHORED.iter().any(|id| value.contains(id)) {
                    continue;
                }
                let line = code[..at].matches('\n').count() + 1;
                if value.contains("filename_component") {
                    guarded.push(format!("{path}:{line}"));
                } else {
                    unguarded.push(format!("{path}:{line}"));
                }
            }
        }
        // The control comes first and counts **sites**, not guarded ones: a
        // needle that stops matching must not read as a clean tree, and a site
        // that loses its guard must be reported as unguarded rather than
        // disappearing out of the count.
        assert_eq!(
            guarded.len() + unguarded.len(),
            4,
            "this tree has three production sites that build a filename stem from a \
             plan-authored id — `coordinator.rs`'s `let stem = …` on the live legacy path and \
             `assembly.rs`'s two field initializers — plus the `<self-check>` corpus entry, and \
             the census found {} ({guarded:?}, {unguarded:?}). An equality rather than a floor, because a fourth site has to be \
             read once by a person, and because a needle that quietly stops matching is how this \
             census came to miss the legacy site entirely",
            guarded.len() + unguarded.len()
        );
        assert!(
            unguarded.is_empty(),
            "these sites put a plan-authored `display_id` into a filename stem \
             without `util::filename_component`, so an `id=` annotation \
             containing a path separator or `..` reaches `std::fs::write`: \
             {unguarded:?}"
        );
    }

    /// **Every field of a reviewer's `WorkerProfile` is accounted for at both
    /// callers.**
    ///
    /// The seventh single-authority census, and the one that closes the class
    /// `PR7-R3-ATTEMPT-001` opened: **the extraction dropped something the
    /// legacy caller supplied.** Three instances now — the sanitiser on a task
    /// id (a **guard**), the reviewer's pool and the retry's pool (**values**) —
    /// found one at a time by three different reviewers.
    ///
    /// # The field list comes from the type
    ///
    /// PR4's rule, and the reason this census cannot sprawl: the roll is
    /// `crate::ir::WorkerProfile`'s own fields, not a list somebody thought of.
    /// Adding a field to that struct fails this test until the new field is
    /// given a cell, which is the property a census of "did we forget anything"
    /// has to have to mean anything.
    ///
    /// # Three cells, and every field has exactly one
    ///
    /// - **Identical** — both callers supply the same value by the same route.
    /// - **Differs, cited** — the callers legitimately disagree, and the cell
    ///   carries the §-citation for why. `pool` is the model: §11.3/§13 make a
    ///   cross-vendor second opinion draw on its own subscription, so it is
    ///   looked up from the reviewer's agent rather than inherited.
    /// - **Absent, cited** — neither caller sets it beyond the constructor's
    ///   default, and the cell says what supplies it instead.
    ///
    /// A cell is prose and this test cannot check prose. What it checks is that
    /// **the roll is complete** — that no field of the type is missing a cell —
    /// which is exactly the failure all three instances share: nobody had
    /// enumerated what the legacy caller supplies.
    #[test]
    fn a_reviewers_profile_is_accounted_for_at_both_callers() {
        use std::collections::BTreeSet;

        /// (field, cell).
        const ROLL: &[(&str, &str)] = &[
            (
                "name",
                "IDENTICAL — `ReviewPass::profile` builds `{lens}-{model}` for both.",
            ),
            (
                "agent",
                "IDENTICAL — the pass's own binding, at both callers.",
            ),
            (
                "model",
                "IDENTICAL — the pass's own binding, at both callers.",
            ),
            (
                "pool",
                "DIFFERS, CITED — §11.3/§13: a cross-vendor second opinion draws on a different                  subscription than the implementer, so the pool is looked up from the reviewer's                  OWN agent rather than inherited. `coordinator.rs` does this via `pool_name_for`;                  `assembly.rs` via `capacity::pool_for` over its frozen pool table. Same rule, two                  lookups, because the two callers hold the table differently. **This is the field                  the extraction dropped** — `assembly.rs` left the constructor's empty string, so                  a schema-4 reviewer drained a pool with no name (Sol, round 3).",
            ),
            (
                "permissions",
                "IDENTICAL — `PermissionMode::ReadOnly` from `profile_for`. A reviewer never                  writes, and neither caller overrides it.",
            ),
            (
                "effort",
                "DIFFERS, CITED — §10's review axis is separate from the implementation rungs.                  `coordinator.rs` passes `self.effort_policy.review`; `assembly.rs` passes                  `entry.ladder.effort.review`, the same policy frozen onto the entry. The values                  agree; the routes differ because a resumed run reads its policy from the record                  rather than from today's config.",
            ),
            (
                "max_turns",
                "ABSENT, CITED — `profile_for` leaves it `None` and neither caller sets it. A                  review pass is one shot with a pass timeout (`ReviewPlan::pass_timeout_secs`),                  not a turn budget.",
            ),
            (
                "extra_args",
                "ABSENT, CITED — `profile_for` leaves it empty and neither caller sets it. Extra                  arguments are an implementer affordance; a reviewer's command is assembled from                  its lens and its inputs.",
            ),
        ];

        // The roll is checked against the TYPE, not against itself.
        let ir = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ir.rs"),
        )
        .expect("the ir source");
        let start = ir
            .find("pub struct WorkerProfile {")
            .expect("`WorkerProfile` is declared in `src/ir.rs`");
        // Past the declaration line: `pub struct WorkerProfile {` itself starts
        // with `pub ` and would otherwise parse as a field.
        let open = start + ir[start..].find('{').expect("an opening brace") + 1;
        let body = &ir[open..open + ir[open..].find("\n}").expect("a closing brace")];
        let fields: BTreeSet<String> = body
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub "))
            .filter_map(|rest| rest.split(':').next())
            .map(str::to_owned)
            .collect();

        assert!(
            fields.len() >= 6,
            "only {} fields parsed out of `WorkerProfile`, so this census is reading the wrong              thing: {fields:?}",
            fields.len()
        );

        let rolled: BTreeSet<String> = ROLL.iter().map(|(f, _)| (*f).to_owned()).collect();
        assert_eq!(
            rolled, fields,
            "the roll and `WorkerProfile`'s fields disagree. A field the type has and the roll              does not is a field nobody has asked whether both callers supply — which is how the              reviewer's `pool` was dropped by the extraction and found three rounds later"
        );
    }

    #[test]
    fn every_production_command_spec_payload_is_classified() {
        use std::collections::BTreeMap;

        /// (file, `.stdin(`, `.env(`, struct-literal `env:`/`stdin:`, and what
        /// they are).
        const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[
            (
                "src/agent/bin.rs",
                0,
                0,
                2,
                "`Invocation::spec` — one of the crate's two CommandSpec \
                 constructors. Both payload fields are `Vec::new()`, and \
                 `a_command_specs_payload_does_not_depend_on_its_arguments` is \
                 what says they stay constant over production's own argument \
                 vectors. Invisible to the method-call columns, which is why \
                 this column exists (PR5-FIDELITY-001)",
            ),
            (
                "src/agent/proc.rs",
                1,
                1,
                0,
                "the process funnel's own `Command`: `.stdin(Stdio::piped())` \
                 is the pipe it writes the payload into, and the `.env` is the \
                 reaper's `/bin/ps` query on macOS. Neither is a CommandSpec",
            ),
            (
                "src/engine/assembly.rs",
                1,
                0,
                0,
                "the worker's prompt: `CommandSpec::stdin` from \
                 `AgentAdapter::stdin_payload`. The role grid carries it. \
                 **Moved from `src/engine/attempt.rs`**, same count, when the \
                 worker's command assembly was lifted out of the legacy \
                 engine so the schema-4 driver could be its second *caller* \
                 rather than its second implementation. This census is the \
                 evidence that the move preserved behaviour: it reported one \
                 entry changing file and every other row identical",
            ),
            (
                "src/gates.rs",
                0,
                0,
                2,
                "`ShellKind::spec` — the crate's other CommandSpec \
                 constructor, and the same answer: two `Vec::new()` payload \
                 fields, held constant by the same test",
            ),
            (
                "src/review.rs",
                1,
                0,
                0,
                "the reviewer's prompt, from the same seam. The role grid \
                 carries it",
            ),
            (
                "src/workspace.rs",
                1,
                4,
                0,
                "authoritative Git, which DESIGN.md:612 keeps off the boundary \
                 entirely: `std::process::Command` methods on git invocations, \
                 not a CommandSpec",
            ),
            (
                "src/workspace_manager.rs",
                2,
                6,
                0,
                "authoritative Git again, and the same answer: \
                 `std::process::Command` methods on git invocations, never a \
                 CommandSpec. The two `.stdin(` are `Stdio::null()` on the two \
                 builders — these funnels feed no payload to a child — and the \
                 six `.env(` are the fixed author/committer identity and dates \
                 that make a commit-tree a function of its inputs rather than \
                 of the machine",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for (relative, code) in production_sources() {
            let counts = (
                code.matches(".stdin(").count(),
                code.matches(".env(").count(),
                // Struct-literal initializers of the same two fields. Anchored
                // at the start of a line so `.env(` chains and doc prose cannot
                // contribute, and counted separately so a row says which kind
                // of population it is.
                code.lines()
                    .filter(|line| {
                        let line = line.trim_start();
                        line.starts_with("env:") || line.starts_with("stdin:")
                    })
                    .count(),
            );
            if counts != (0, 0, 0) {
                found.insert(relative, counts);
            }
        }
        // `PR4-CENSUS-COMMENT-ORACLE`'s class — a census over a file format that
        // has comments. The control that the blanking removed something is in
        // `production_sources`, which every census here shares, rather than
        // repeated four times: it asserts the regions hold strictly fewer
        // non-whitespace bytes than the sources they came from, and a floor
        // beneath which the counts would be over nothing.

        let expected: BTreeMap<String, (usize, usize, usize)> = EXPECTED
            .iter()
            .map(|(file, stdin, env, literal, _)| ((*file).to_owned(), (*stdin, *env, *literal)))
            .collect();
        assert_eq!(
            found, expected,
            "a production call site populates a CommandSpec payload field. Classify it, and if \
             it is a spec field, make the fixture grids carry that shape — an observer \
             suppression keyed on a field no grid varies is invisible (PR4-CONF-006), and one \
             keyed on a field no census counts is invisible twice (PR5-FIDELITY-001)"
        );
    }

    /// The two spec constructors' payload is a function of nothing.
    ///
    /// DESIGN.md:262-264: "Probe and execution compose the **same** base,
    /// mounts, reserved values, and overlay, so pre-flight certifies the
    /// environment that will actually spend." A probe and a work command differ
    /// in exactly one thing — their **arguments** — so an overlay that varies
    /// with the arguments is an overlay that differs between pre-flight and
    /// spend, and `PR5-FIDELITY-001` is that edit at `bin::Invocation::spec`.
    ///
    /// The census above says a site *exists*; this says what it produces. Both
    /// are needed and neither implies the other: a census cannot tell
    /// `Vec::new()` from a conditional, and a fixture that built one spec
    /// cannot tell a constant from a function of its input.
    ///
    /// The argument vectors are production's own — every adapter's `--version`
    /// probe, every adapter's `build_args` fresh and resumed, Codex's six
    /// strict-config parser probes' shape, and the gate/shell dialects — so
    /// this is a statement about the values production actually passes and not
    /// about invented ones.
    #[test]
    fn a_command_specs_payload_does_not_depend_on_its_arguments() {
        use crate::agent::bin::Invocation;

        fn run(agent: &str, resume: Option<&str>) -> crate::agent::TaskRun {
            crate::agent::TaskRun {
                prompt: "Do the thing.".to_owned(),
                profile: crate::ir::WorkerProfile {
                    name: "impl-mid".to_owned(),
                    agent: agent.to_owned(),
                    model: "a-model".to_owned(),
                    pool: "a-pool".to_owned(),
                    permissions: crate::ir::PermissionMode::ReadOnly,
                    effort: Some(crate::ir::Effort::Medium),
                    max_turns: Some(30),
                    extra_args: Vec::new(),
                },
                workspace: PathBuf::from("."),
                gate_cmds: Vec::new(),
                resume_session: resume.map(str::to_owned),
                settings_path: None,
            }
        }

        let mut argument_vectors: Vec<Vec<String>> = vec![
            vec!["--version".to_owned()],
            vec!["--help".to_owned()],
            vec!["exec".to_owned(), "--help".to_owned()],
            vec![
                "exec".to_owned(),
                "--ignore-user-config".to_owned(),
                "--strict-config".to_owned(),
                "-c".to_owned(),
                "model_reasoning_effort=xhigh".to_owned(),
            ],
            vec!["login".to_owned(), "status".to_owned()],
            vec!["debug".to_owned(), "models".to_owned()],
            Vec::new(),
        ];
        for id in ["claude-code", "codex", "copilot"] {
            for resume in [None, Some("session-1")] {
                argument_vectors.push(match id {
                    "claude-code" => crate::agent::claude::build_args(&run(id, resume)),
                    "codex" => crate::agent::codex::build_args(&run(id, resume)),
                    _ => crate::agent::copilot::build_args(&run(id, resume)),
                });
            }
        }
        assert!(
            argument_vectors.len() >= 13,
            "the argument vectors are production's own: {}",
            argument_vectors.len()
        );
        assert!(
            argument_vectors
                .iter()
                .any(|args| args.first().is_some_and(|arg| arg == "--version")),
            "a probe's argument vector must be among them, or the claim is untested"
        );
        assert!(
            argument_vectors
                .iter()
                .any(|args| args.first().is_some_and(|arg| arg == "exec")),
            "and a work command's"
        );

        // (a) `bin::Invocation::spec`, the agent-CLI constructor.
        let invocation = Invocation::at(if cfg!(windows) {
            r"C:\nowhere\claude.cmd"
        } else {
            "/nowhere/claude"
        });
        /// One spec's payload: its overlay and its stdin.
        type Payload = (Vec<(String, String)>, Vec<u8>);
        let mut payloads: Vec<Payload> = Vec::new();
        for args in &argument_vectors {
            let spec = invocation.spec(args).expect("a Unicode path");
            assert_eq!(&spec.args, args, "the arguments are carried verbatim");
            payloads.push((spec.env, spec.stdin));
        }
        let first = payloads.first().expect("at least one vector").clone();
        for (index, payload) in payloads.iter().enumerate() {
            assert_eq!(
                payload, &first,
                "`Invocation::spec` gave argument vector {index} ({:?}) a different \
                 payload than it gave {:?} — pre-flight would then certify an \
                 environment other than the one that spends",
                argument_vectors[index], argument_vectors[0]
            );
        }
        assert_eq!(
            first,
            (Vec::new(), Vec::new()),
            "and the payload production's constructor writes is empty"
        );

        // (b) `gates::ShellKind::spec`, the other one. Every dialect, because
        // the shell is a field of the record and not a constant.
        let mut shell_payloads: Vec<Payload> = Vec::new();
        use crate::gates::ShellKind;
        for shell in [
            ShellKind::Cmd,
            ShellKind::Sh,
            ShellKind::Bash,
            ShellKind::PowerShell,
            ShellKind::Pwsh,
        ] {
            for line in ["exit 0", "cargo test --all", "echo \"quoted arg\""] {
                let spec = shell.spec(line);
                shell_payloads.push((spec.env, spec.stdin));
            }
        }
        assert_eq!(
            shell_payloads.len(),
            15,
            "five dialects, three command lines"
        );
        for payload in &shell_payloads {
            assert_eq!(
                payload,
                &(Vec::new(), Vec::new()),
                "`ShellKind::spec` populated a payload field"
            );
        }
    }

    #[test]
    fn harness_hooks_consult_every_mode_a_point_declares() {
        let harness = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = HarnessHooks::new(Arc::clone(&harness));
        for point in [
            SubEffectPoint::AmbientJobJoined,
            SubEffectPoint::CreatedSuspended,
        ] {
            assert_eq!(hooks.point(point), Injection::Proceed);
        }
        let harness = harness.lock().expect("harness");
        // AmbientJobJoined declares both modes; CreatedSuspended declares kill
        // only. The expected pairs come from `containment_sub_effects` ("failure
        // refuses the write command" for the ambient join alone), not from
        // `SubEffectPoint::modes`.
        assert!(harness.reached_point(
            SPAWN_SITE,
            SubEffectPoint::AmbientJobJoined,
            InjectionMode::Kill
        ));
        assert!(harness.reached_point(
            SPAWN_SITE,
            SubEffectPoint::AmbientJobJoined,
            InjectionMode::ErrorReturn
        ));
        assert!(harness.reached_point(
            SPAWN_SITE,
            SubEffectPoint::CreatedSuspended,
            InjectionMode::Kill
        ));
        assert!(!harness.reached_point(
            SPAWN_SITE,
            SubEffectPoint::CreatedSuspended,
            InjectionMode::ErrorReturn
        ));
    }
}
