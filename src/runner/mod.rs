//! Extended notes: `docs/internals/runner/mod.md`

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Vec<u8>,
}

impl CommandSpec {
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = stdin.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentId(String);

impl AgentId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProbeTarget {
    Agent(AgentId),
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionRole {
    Probe(ProbeTarget),
    Implement,
    Gate,
    Review,
}

impl ExecutionRole {
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

    #[must_use]
    pub fn is_slotted(&self) -> bool {
        match self {
            Self::Probe(ProbeTarget::Agent(_)) | Self::Implement | Self::Review => true,
            Self::Probe(ProbeTarget::Shell) | Self::Gate => false,
        }
    }

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

#[derive(Debug, Clone)]
pub struct RunnerRequest {
    pub command: CommandSpec,
    pub workspace: PathBuf,
    pub role: ExecutionRole,
    pub timeout: Duration,
    pub agent: Option<AgentId>,
    pub invocation: InvocationId,
}

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
        role: ExecutionRole::Implement,
        timeout,
        agent: Some(agent),
        invocation,
    }
}

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

pub trait Runner: Send + Sync {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError>;
}

pub const SPAWN_SITE: EffectSiteId = EffectSiteId::Process(ProcessSite::Spawn);

#[derive(Debug, Clone, Default)]
pub struct HarnessHooks {
    harness: Arc<Mutex<HookHarness>>,
}

impl HarnessHooks {
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self { harness }
    }

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
            assert_eq!(request.command, spec, "{role}: the command spec");
            assert_eq!(request.workspace, workspace, "{role}");
            assert_eq!(request.timeout, timeout, "{role}");
        }
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
        let labels: std::collections::BTreeSet<String> = ExecutionRole::all()
            .iter()
            .map(ExecutionRole::label)
            .collect();
        assert_eq!(labels.len(), ExecutionRole::all().len());
    }

    #[test]
    fn the_runner_trait_is_object_safe() {
        fn takes_dyn(_: &dyn Runner) {}
        let runner = host::HostRunner::new();
        takes_dyn(&runner);
        let boxed: Box<dyn Runner> = Box::new(host::HostRunner::new());
        takes_dyn(boxed.as_ref());
    }

    #[test]
    fn invocation_ids_are_unique_within_a_run_incl_agent_and_shell_probes() {
        const TASKS: u32 = 7;
        const ATTEMPTS: u32 = 3;
        const GATES: u32 = 4;
        const SEQUENCES: u32 = 2;
        const AGENTS: [&str; 3] = ["claude-code", "copilot", "codex"];

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
            for sequence in 0..SEQUENCES {
                push(
                    ExecutionRole::Gate,
                    None,
                    InvocationId::sequence(SequenceId(sequence), SequenceRole::Gate(0), 0),
                );
                push(
                    ExecutionRole::Review,
                    Some(AgentId::new(AGENTS[(sequence % 3) as usize])),
                    InvocationId::sequence(SequenceId(sequence), SequenceRole::ReviewPass(0), 0),
                );
            }
            requests
        }

        let requests = run_requests();
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
        let counted = |prefix: &str| ids.iter().filter(|id| id.starts_with(prefix)).count();
        assert_eq!(counted("p."), 1 + AGENTS.len(), "the pre-flight probes");
        assert_eq!(
            counted("k"),
            (TASKS * ATTEMPTS * (1 + GATES + 1)) as usize,
            "the attempt form"
        );
        assert_eq!(counted("s"), (SEQUENCES * 2) as usize, "the sequence form");

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

    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct ParsedRow {
        pub(crate) name: &'static str,
        pub(crate) adapter: &'static str,
        pub(crate) status: crate::ir::OutcomeStatus,
        pub(crate) detail: Option<String>,
        pub(crate) session: Option<String>,
        pub(crate) cost_usd: Option<f64>,
    }

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

    #[test]
    fn the_spawn_site_files_every_role_under_one_context_and_the_count_says_which() {
        use crate::topology::effects::{Adjacent, DurableEvent, FaultRow, ObservableOrder};

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

    #[test]
    fn the_containment_coordinates_are_pinned_against_written_literals() {
        use crate::topology::effects::ProcessSite;

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
            let decoded: SubEffectPoint =
                serde_json::from_str(wire).expect("decode the written literal");
            assert_eq!(decoded, *point, "{coordinate} no longer accepts {wire}");
        }
    }

    fn receiver_writes(code: &str, field: &str) -> usize {
        let needle = format!(".{field}");
        code.match_indices(&needle)
            .filter(|(at, _)| {
                let rest = code[at + needle.len()..].trim_start();
                match rest.as_bytes() {
                    [b'=', b'=', ..] => false,
                    [b'=', ..] => true,
                    [b'<', b'<', b'=', ..] | [b'>', b'>', b'=', ..] => true,
                    [op, b'=', ..] => {
                        matches!(op, b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^')
                    }
                    _ => false,
                }
            })
            .count()
    }

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

    fn function_body<'a>(code: &'a str, name: &str) -> Option<&'a str> {
        let needle = format!("fn {name}(");
        let mut definitions = code.match_indices(&needle);
        let (found, _) = definitions.next()?;
        assert!(
            definitions.next().is_none(),
            "`{name}` is defined more than once in this region, so a body-wise count would \
             read only the first"
        );
        let bytes = code.as_bytes();
        let mut at = found + needle.len() - 1;
        let mut round = 0_i32;
        let mut square = 0_i32;
        let open = loop {
            match bytes.get(at)? {
                b'(' => round += 1,
                b')' => round -= 1,
                b'[' => square += 1,
                b']' => square -= 1,
                b';' if round == 0 && square == 0 => return None,
                b'{' if round == 0 && square == 0 => break at,
                _ => {}
            }
            at += 1;
        };
        let mut depth = 0_i32;
        let mut end = open;
        loop {
            match bytes.get(end)? {
                b'{' => depth += 1,
                b'}' if depth == 1 => return Some(&code[open + 1..end]),
                b'}' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
    }

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

    fn production_sources() -> Vec<(String, String)> {
        production_sources_by_path()
            .into_iter()
            .map(|(path, code)| (display_path(&path), code))
            .collect()
    }

    fn display_path(relative: &std::path::Path) -> String {
        relative.to_string_lossy().replace('\\', "/")
    }

    fn production_sources_by_path() -> Vec<(PathBuf, String)> {
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
        let test_modules =
            crate::effects::census_domain::whole_file_test_modules(&root.join("src"), &files, 13);
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
        let sources: Vec<(PathBuf, String)> = files
            .into_iter()
            .filter_map(|path| {
                if test_modules.contains(&path) {
                    return None;
                }
                let relative = path
                    .strip_prefix(&root)
                    .expect("under the manifest")
                    .to_path_buf();
                let source = std::fs::read_to_string(&path).expect("read source");
                raw_bytes += dense(&source);
                Some((relative, crate::effects::production_code(&source)))
            })
            .collect();
        for (relative, code) in &sources {
            let relative = display_path(relative);
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
        assert!(
            region_bytes < raw_bytes,
            "the sources hold {raw_bytes} non-whitespace bytes and the regions hold \
             {region_bytes}; the blanking removed nothing, so the counts below are over prose"
        );
        sources
    }

    #[test]
    fn every_production_runner_request_is_built_by_its_roles_builder() {
        use std::collections::BTreeMap;

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
        for absent in [
            "src/gates.rs",
            "src/review.rs",
            "src/engine/attempt.rs",
            "src/engine/coordinator.rs",
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

        for (file, why) in [
            (
                "src/events/mod.rs",
                "the event vocabulary and fold: DESIGN.md:612 puts the event log, \
                 with authoritative Git, among the things that never cross the \
                 boundary",
            ),
            (
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

        for adapter in [
            "src/agent/mod.rs",
            "src/agent/bin.rs",
            "src/agent/claude.rs",
            "src/agent/copilot.rs",
            "src/agent/codex.rs",
        ] {
            let source = std::fs::read_to_string(root.join(adapter)).expect("read adapter");
            let code = crate::effects::production_code(&source);
            assert_eq!(
                count(&code, "HostRunner"),
                0,
                "{adapter} names a concrete boundary; an adapter receives one"
            );
        }
    }

    #[test]
    fn write_command_containment_has_one_join_site_and_one_mint() {
        use std::collections::BTreeMap;

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

    #[test]
    fn a_command_is_assembled_in_one_production_place_per_role() {
        use std::collections::BTreeMap;

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

    #[test]
    fn an_attempts_ledger_line_is_constructed_in_one_production_place() {
        use std::collections::BTreeMap;

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

    #[test]
    fn a_worker_profile_is_constructed_in_one_production_place_per_role() {
        use std::collections::BTreeMap;

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

    #[test]
    fn an_observation_about_an_attempt_is_classified_in_one_production_place() {
        use std::collections::BTreeMap;

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
            (
                "src/status/render.rs",
                0,
                3,
                "reads an outcome to say, in a `--follow` line, whether a \
                 review pass rejected the code or never reached a verdict. A \
                 reader, like `export.rs`: this changes display text, not \
                 run state. Schema-3 and schema-4 success validation use \
                 `is_successful`; this reader renders the supplied record",
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

    #[test]
    fn the_rungs_allowance_is_counted_in_one_production_place() {
        use std::collections::BTreeMap;

        const EXPECTED: &[(&str, usize, usize, &str)] = &[
            (
                "src/events/mod.rs",
                7,
                0,
                "**the legacy schema-3 progress tracker, and the reason this census exists rather than a bare repair.** It counts an attempt at its *start* and refunds by SUBTRACTION — five `saturating_sub` sites against one `saturating_add`, plus two resets. Each of those five is a place a future refund can be forgotten, which is the bug schema 4 shipped. **Recorded, not unified**: this is the legacy engine's own in-memory state, `invariants_preserved[1]` freezes its behaviour, and rewriting it would change the engine actually in production to tidy the one that is not. Zero consults is the finding in one number — the legacy engine never asks `spends_allowance`, because the rule was extracted FROM it",
            ),
            (
                "src/topology/fold/apply.rs",
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

        use std::path::Path;

        let fold_root = Path::new("src/topology/fold.rs");
        let fold_dir = Path::new("src/topology/fold");
        let walked = production_sources_by_path();
        let children = walked
            .iter()
            .filter(|(path, _)| path.parent() == Some(fold_dir))
            .count();
        assert!(
            children >= 10,
            "the walk found {children} production children of the fold, so the counts below are \
             over almost nothing"
        );

        let charge_calls = |code: &str| {
            use crate::effects::census_domain::{Call, production_calls};
            production_calls(code, "charge_allowance", Call::Free)
                + production_calls(code, "charge_allowance", Call::Method)
        };
        const SPELLINGS: &str = "\
fn control() {
    self.charge_allowance(key, record);
    Self::charge_allowance(self, key, record);
    RunState::charge_allowance(self, key, record);
    crate::topology::fold::RunState::charge_allowance(self, key, record);
    self.recharge_allowance(key, record);
    self.charge_allowance_twice(key, record);
}
fn charge_allowance(&mut self) {}
";
        let spelled = charge_calls(SPELLINGS);
        assert_eq!(
            spelled, 4,
            "the counter reads {spelled} of the four spellings this fixture carries, so an \
             applier could empty the map below by rewriting the one spelling it does read"
        );

        const APPLIERS: [&str; 2] = ["apply_settlement", "apply_candidate_prepared"];
        let mut charging: BTreeMap<&str, usize> = BTreeMap::new();
        let mut charges = 0;
        for (path, code) in &walked {
            if path.as_path() != fold_root && !path.starts_with(fold_dir) {
                continue;
            }
            charges += charge_calls(code);
            for applier in APPLIERS {
                let Some(body) = function_body(code, applier) else {
                    continue;
                };
                assert!(
                    charging.insert(applier, charge_calls(body)).is_none(),
                    "`{applier}` is defined in two files of the fold's production region, so \
                     which of them settles is not decided here"
                );
            }
        }
        assert_eq!(
            charging,
            APPLIERS
                .iter()
                .map(|name| (*name, 1))
                .collect::<BTreeMap<_, _>>(),
            "each settlement applier's body names `charge_allowance` exactly once, and the map \
             on the left is what the fold's production region actually spells. This half is \
             lexical and says nothing about what the appliers CALL; \
             `topology::fold::tests::an_interrupted_attempt_refunds_the_rungs_allowance` is the \
             half that reads the charge off the state. A `0` here is an applier whose body no \
             longer names the charge at all — the shape of the 2026-08-27 defect, an operator \
             handed a free attempt on a rung already paid for; a `2` is one applier naming it \
             twice; a missing applier is one that has been renamed or has left the fold, and \
             this census has stopped reading the code it names"
        );
        assert_eq!(
            charges, 2,
            "`charge_allowance` is named {charges} time(s) in the fold module's production \
             region; the two settlement appliers account for two, so any other number is a \
             naming outside both of them"
        );
    }

    #[test]
    fn each_census_needle_covers_the_domain_its_doc_states() {
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

        let field = stem_values("Inputs { stem: format!(\"{i:02}-{}\", display_id), n: 1 }");
        assert_eq!(field.len(), 1, "{field:?}");
        assert!(
            field[0].1.contains("display_id"),
            "the value was truncated at the comma inside `format!`, so the site is skipped \
             rather than judged: {:?}",
            field[0].1
        );

        let binding =
            stem_values("let stem = format!(\"{i:02}-{}\", filename_component(task.id));\n");
        assert_eq!(binding.len(), 1, "{binding:?}");
        assert!(
            binding[0].1.contains("task.id") && binding[0].1.contains("filename_component"),
            "{:?}",
            binding[0].1
        );

        assert!(
            stem_values("let file_stem = path.file_stem();").is_empty(),
            "`file_stem` is not a filename stem built from a task id"
        );
    }

    #[test]
    fn a_plan_authored_id_never_reaches_a_filename_unsanitised() {
        const PLAN_AUTHORED: &[&str] = &["display_id", "task.id"];

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

    #[test]
    fn a_reviewers_profile_is_accounted_for_at_both_callers() {
        use std::collections::BTreeSet;

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

        let ir = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ir.rs"),
        )
        .expect("the ir source");
        let start = ir
            .find("pub struct WorkerProfile {")
            .expect("`WorkerProfile` is declared in `src/ir.rs`");
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
                7,
                0,
                "authoritative Git again, and the same answer: \
                 `std::process::Command` methods on git invocations, never a \
                 CommandSpec. The two `.stdin(` are `Stdio::null()` on the two \
                 builders — these funnels feed no payload to a child — and of \
                 the seven `.env(`, six are the fixed author/committer identity \
                 and dates that make a commit-tree a function of its inputs \
                 rather than of the machine, and the seventh is \
                 `GIT_NO_REPLACE_OBJECTS=1` on the shared builder, so that the \
                 commands this manager itself spawns read the objects the \
                 repository holds rather than whatever `git replace` has been \
                 pointed at them — a gate or reviewer process gets the \
                 environment this runner composes, which is a separate question \
                 and a deferred row",
            ),
        ];

        let mut found: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
        for (relative, code) in production_sources() {
            let counts = (
                code.matches(".stdin(").count(),
                code.matches(".env(").count(),
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

        let invocation = Invocation::at(if cfg!(windows) {
            r"C:\nowhere\claude.cmd"
        } else {
            "/nowhere/claude"
        });
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
