//! Extended notes: `docs/internals/agent/mod.md`

pub mod bin;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod proc;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capacity::PoolKind;
use crate::error::UpstrokeError;
use crate::ir::{Effort, Outcome, WorkerProfile};
use crate::runner::invocation::InvocationId;
use crate::runner::{AgentId, CommandSpec, ExecutionRole, ProbeTarget, Runner, RunnerRequest};

pub use proc::ProcessOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    Authenticated,
    NotAuthenticated,
    Unknown,
}

impl std::fmt::Display for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Authenticated => "signed in",
            Self::NotAuthenticated => {
                "NOT signed in — log in with the vendor's own CLI before running"
            }
            Self::Unknown => "auth state could not be determined",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub auth: AuthState,
    pub models: Vec<String>,
    pub shape: Option<PoolKind>,
    pub notes: Vec<String>,
}

impl Discovery {
    pub fn unknown() -> Self {
        Self {
            auth: AuthState::Unknown,
            models: Vec::new(),
            shape: None,
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caps {
    pub version: String,
    pub json_output: bool,
    pub session_resume: bool,
    pub cost_reporting: bool,
    pub read_only_mode: bool,
    pub acp: bool,
    pub model_list: bool,
}

#[derive(Debug, Clone)]
pub struct TaskRun {
    pub prompt: String,
    pub profile: WorkerProfile,
    pub workspace: PathBuf,
    pub gate_cmds: Vec<String>,
    pub resume_session: Option<String>,
    pub settings_path: Option<PathBuf>,
}

#[must_use]
pub fn probe_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn probe_request(
    agent: &str,
    command: CommandSpec,
    ordinal: u32,
    timeout: Duration,
) -> Result<RunnerRequest, UpstrokeError> {
    let agent = AgentId::new(agent);
    Ok(RunnerRequest {
        command,
        workspace: probe_workspace(),
        role: ExecutionRole::Probe(ProbeTarget::Agent(agent.clone())),
        timeout,
        agent: Some(agent.clone()),
        invocation: InvocationId::probe(ProbeTarget::Agent(agent), ordinal)?,
    })
}

pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn probe(&self, runner: &dyn Runner) -> Result<Caps, UpstrokeError>;
    fn build(&self, run: &TaskRun) -> Result<CommandSpec, UpstrokeError>;
    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, UpstrokeError>;

    fn discover(&self, _runner: &dyn Runner, _caps: &Caps) -> Result<Discovery, UpstrokeError> {
        Ok(Discovery::unknown())
    }

    fn stdin_payload<'a>(&self, run: &'a TaskRun) -> &'a str {
        &run.prompt
    }

    fn materialize_permissions(
        &self,
        _profile: &WorkerProfile,
        _gate_cmds: &[String],
        _dir: &std::path::Path,
        _stem: &str,
    ) -> Result<Option<PathBuf>, UpstrokeError> {
        Ok(None)
    }
}

pub trait AdapterSource {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter>;
}

pub struct BuiltinAdapters;

impl AdapterSource for BuiltinAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        by_id(id).map(|a| a as &dyn AgentAdapter)
    }
}

pub static ADAPTERS: &[&dyn AgentAdapter] = &[
    &claude::ClaudeCodeAdapter,
    &copilot::CopilotAdapter,
    &codex::CodexAdapter,
];

pub fn by_id(id: &str) -> Option<&'static dyn AgentAdapter> {
    ADAPTERS.iter().copied().find(|a| a.id() == id)
}

pub(crate) fn missing_effort_levels(help: &str) -> Vec<Effort> {
    let mut block = String::new();
    let mut collecting = false;
    for line in help.lines() {
        if !collecting {
            if line.contains("--effort") {
                collecting = true;
                block.push_str(line);
                block.push('\n');
            }
            continue;
        }
        if line.trim_start().starts_with('-') {
            break;
        }
        block.push_str(line);
        block.push('\n');
    }
    if !collecting {
        return Effort::ALL.to_vec();
    }

    let advertised: Vec<Effort> = block
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter_map(Effort::parse)
        .collect();
    Effort::ALL
        .into_iter()
        .filter(|effort| !advertised.contains(effort))
        .collect()
}

pub(crate) fn advertises_flag(help: &str, flag: &str) -> bool {
    help.split(|character: char| character.is_whitespace() || character == ',')
        .map(|token| token.split(['=', ':']).next().unwrap_or(token))
        .any(|name| name == flag)
}

pub fn looks_rate_limited(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "usage limit",
        "rate limit",
        "rate_limit",
        "limit reached",
        "limit exceeded",
        "overloaded",
        "quota exceeded",
        "insufficient credits",
        "out of credits",
        "premium request",
        "monthly limit",
        "429",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_every_shipped_adapter() {
        assert!(by_id("claude-code").is_some());
        assert!(by_id("copilot").is_some());
        assert!(by_id("codex").is_some());
        assert!(by_id("aider").is_none(), "aider arrives in v0.2");
    }

    fn a_successful_answer_from(id: &str) -> String {
        match id {
            "claude-code" => {
                r#"{"session_id":"s-1","total_cost_usd":0.5,"result":"done","subtype":"success"}"#
                    .to_owned()
            }
            "copilot" => "done".to_owned(),
            "codex" => [
                r#"{"type":"thread.started","thread_id":"th-1"}"#,
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
                r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}"#,
            ]
            .join("\n"),
            other => panic!("an adapter shipped without an answer shape here: {other}"),
        }
    }

    #[test]
    fn every_adapter_maps_every_supervision_result_the_same_way() {
        use crate::ir::OutcomeStatus;

        type SupervisionCell = (
            &'static str,
            Option<i32>,
            bool,
            bool,
            OutcomeStatus,
            &'static str,
        );
        const GRID: &[SupervisionCell] = &[
            (
                "clean success",
                Some(0),
                false,
                false,
                OutcomeStatus::Completed,
                "done",
            ),
            (
                "output-limited although it exited 0",
                Some(0),
                false,
                true,
                OutcomeStatus::AgentError,
                "output limit",
            ),
            (
                "output-limited and terminated",
                None,
                false,
                true,
                OutcomeStatus::AgentError,
                "output limit",
            ),
            (
                "timed out although it exited 0",
                Some(0),
                true,
                false,
                OutcomeStatus::Timeout,
                "wall-clock timeout",
            ),
            (
                "timed out and terminated",
                None,
                true,
                false,
                OutcomeStatus::Timeout,
                "wall-clock timeout",
            ),
            (
                "an ordinary non-zero exit",
                Some(1),
                false,
                false,
                OutcomeStatus::AgentError,
                "",
            ),
        ];

        let mut statuses: Vec<OutcomeStatus> = Vec::new();
        let mut cells = 0_usize;
        for adapter in ADAPTERS {
            let stdout = a_successful_answer_from(adapter.id());
            for (name, code, timed_out, output_limited, expected, must_carry) in GRID {
                let out = ProcessOutput {
                    code: *code,
                    stdout: stdout.clone(),
                    stderr: String::new(),
                    duration: Duration::from_millis(9),
                    timed_out: *timed_out,
                    output_limited: *output_limited,
                };
                let cell = format!("{}/{name}", adapter.id());
                let outcome = adapter
                    .parse(&out)
                    .unwrap_or_else(|error| panic!("{cell}: parse: {error}"));
                assert_eq!(outcome.status, *expected, "{cell}: wrong status");
                if !must_carry.is_empty() {
                    let detail = outcome.detail.clone().unwrap_or_default();
                    assert!(
                        detail.contains(must_carry),
                        "{cell}: the detail must say why (`{must_carry}`): {detail:?}"
                    );
                }
                assert_eq!(outcome.duration, out.duration, "{cell}: duration");
                if !statuses.contains(expected) {
                    statuses.push(*expected);
                }
                cells += 1;
            }
        }

        assert_eq!(GRID.len(), 6, "six supervision shapes");
        assert_eq!(cells, 18, "every shipped adapter crossed with every shape");
        assert_eq!(
            statuses.len(),
            3,
            "Completed, AgentError and Timeout are three distinct answers: {statuses:?}"
        );
        assert_ne!(OutcomeStatus::Timeout, OutcomeStatus::AgentError);
    }

    #[test]
    fn an_agent_probe_request_is_slotted_names_its_agent_and_carries_the_probe_identity() {
        use std::path::Path;

        let request = probe_request(
            "claude-code",
            CommandSpec::new("claude").arg("--version"),
            0,
            Duration::from_secs(60),
        )
        .expect("a shipped adapter id survives an invocation identity");

        assert_eq!(
            request.role,
            ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new("claude-code")))
        );
        assert!(
            request.role.is_slotted(),
            "INV-18: an agent probe is slotted"
        );
        assert_eq!(request.agent, Some(AgentId::new("claude-code")));
        assert_eq!(request.invocation.render(), "p.agent-claude-code.o0");
        assert_eq!(
            request.invocation.probe_target(),
            Some(&ProbeTarget::Agent(AgentId::new("claude-code")))
        );
        assert_eq!(request.command.program, "claude");
        assert_eq!(request.workspace, probe_workspace());
        assert!(
            request.workspace.is_absolute() || request.workspace == Path::new("."),
            "the runner is given an absolute directory unless the cwd is gone"
        );

        assert_ne!(request.invocation.render(), "p.shell.o0");
        assert!(
            !ExecutionRole::Probe(ProbeTarget::Shell).is_slotted(),
            "and the shell probe, which this is not, is the non-slotted one"
        );

        let ids: std::collections::BTreeSet<String> = ADAPTERS
            .iter()
            .map(|adapter| {
                probe_request(
                    adapter.id(),
                    CommandSpec::new("x"),
                    0,
                    Duration::from_secs(1),
                )
                .expect("shipped adapter id")
                .invocation
                .render()
            })
            .collect();
        assert_eq!(ids.len(), ADAPTERS.len());
        assert_eq!(ids.len(), 3, "claude-code, copilot, codex");

        let ordinals: std::collections::BTreeSet<String> = (0..4)
            .map(|ordinal| {
                probe_request(
                    "codex",
                    CommandSpec::new("codex"),
                    ordinal,
                    Duration::from_secs(1),
                )
                .expect("shipped adapter id")
                .invocation
                .render()
            })
            .collect();
        assert_eq!(ordinals.len(), 4);
    }

    #[test]
    fn a_probe_request_refuses_an_agent_id_that_would_not_survive_a_container_name() {
        for id in ["claude.code", "clau de", "", "codex/../etc"] {
            assert!(
                probe_request(id, CommandSpec::new("x"), 0, Duration::from_secs(1)).is_err(),
                "`{id}` must not become an invocation identity"
            );
        }
        for id in ["claude-code", "copilot", "codex", "aider_2"] {
            assert!(
                probe_request(id, CommandSpec::new("x"), 0, Duration::from_secs(1)).is_ok(),
                "`{id}` is a legal agent id"
            );
        }
    }

    #[test]
    fn rate_limit_vocabulary_covers_both_vendors() {
        for phrase in [
            "5-hour limit reached ∙ resets 6pm",
            "Weekly limit reached",
            "API error: rate_limit_error",
            "You are out of credits for this month",
            "premium request allowance exhausted",
            "HTTP 429",
        ] {
            assert!(looks_rate_limited(phrase), "should signal: {phrase}");
        }
        assert!(!looks_rate_limited("wrote the pagination cursor encoder"));
    }

    #[test]
    fn effort_help_is_scoped_to_the_effort_option_and_requires_every_level() {
        let claude = "  --effort <level>  Effort level (low, medium, high, xhigh, max)\n\
                      --model <model>   Model to use\n";
        assert_eq!(missing_effort_levels(claude), []);

        let copilot = "  --effort, --reasoning-effort <level>  Reasoning effort \
                       (choices: \"none\", \"minimal\", \"low\", \"medium\", \"high\", \
                       \"xhigh\", \"max\")\n  --model <model>  Model\n";
        assert_eq!(missing_effort_levels(copilot), []);

        let narrower = "  --effort <level>  Effort level (low, medium, high)\n\
                         --other <value>  xhigh and max appear outside the option\n";
        assert_eq!(
            missing_effort_levels(narrower),
            [Effort::XHigh, Effort::Max],
            "another option cannot supply missing effort choices"
        );
    }

    #[test]
    fn short_flags_are_not_inferred_from_longer_names() {
        assert!(advertises_flag("-p, --print", "-p"));
        assert!(!advertises_flag("--permission-mode", "-p"));
        assert!(!advertises_flag("--settings --share --stdio", "-s"));
        assert!(advertises_flag("-c, --config <key=value>", "--config"));
        assert!(!advertises_flag("--configuration <path>", "--config"));
    }
}

#[cfg(test)]
mod built_program_tests {
    use super::*;
    use crate::ir::{Effort, PermissionMode, WorkerProfile};
    use std::sync::{Mutex, PoisonError};

    struct Boundary {
        installed: String,
        version: String,
        seen: Mutex<Vec<CommandSpec>>,
    }

    impl Boundary {
        fn holding(installed: &str) -> Self {
            Self::holding_version(installed, "9.9.9")
        }

        fn holding_version(installed: &str, version: &str) -> Self {
            Self {
                installed: installed.to_owned(),
                version: version.to_owned(),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<CommandSpec> {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        fn programs(&self) -> Vec<String> {
            self.seen().into_iter().map(|spec| spec.program).collect()
        }
    }

    impl Runner for Boundary {
        fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request.command.clone());
            if request.command.program == self.installed {
                let (code, stdout, stderr) =
                    scripted_answer(&self.installed, &request.command.args, &self.version);
                return Ok(ProcessOutput {
                    code: Some(code),
                    stdout,
                    stderr,
                    duration: Duration::from_millis(1),
                    timed_out: false,
                    output_limited: false,
                });
            }
            Err(UpstrokeError::Agent {
                message: format!(
                    "`{}` is not present inside this boundary; the agent CLI here is `{}`",
                    request.command.program, self.installed
                ),
            })
        }
    }

    fn expected_name(id: &str) -> &'static str {
        match id {
            "claude-code" => "claude",
            "copilot" => "copilot",
            "codex" => "codex",
            other => panic!("an adapter shipped without a name in this table: {other}"),
        }
    }

    fn a_run(agent: &str) -> TaskRun {
        TaskRun {
            prompt: "Do the thing.".to_owned(),
            profile: WorkerProfile {
                name: "impl-mid".to_owned(),
                agent: agent.to_owned(),
                model: "a-model".to_owned(),
                pool: "a-pool".to_owned(),
                permissions: PermissionMode::ReadOnly,
                effort: Some(Effort::Medium),
                max_turns: Some(30),
                extra_args: Vec::new(),
            },
            workspace: PathBuf::from("."),
            gate_cmds: Vec::new(),
            resume_session: None,
            settings_path: None,
        }
    }

    fn scripted_help(cli: &str) -> &'static str {
        match cli {
            "claude" => {
                "  -p, --print\n  --output-format <fmt>\n  --model <model>\n  \
                 --effort <level>  Effort level (low, medium, high, xhigh, max)\n  \
                 --settings <file>\n  --setting-sources <src>\n  --permission-mode <mode>\n  \
                 --resume <id>\n"
            }
            "copilot" => {
                "  -s, --stdin\n  --model <model>\n  \
                 --effort <level>  Effort level (low, medium, high, xhigh, max)\n  \
                 --allow-tool <tool>\n  --deny-tool <tool>\n  --no-ask-user\n"
            }
            "codex" => "--json --sandbox --model -c, --config",
            other => panic!("a CLI shipped without a help script: {other}"),
        }
    }

    fn scripted_answer(cli: &str, args: &[String], version: &str) -> (i32, String, String) {
        let joined = args.join(" ");
        if joined == "--version" {
            return (0, format!("{version}\n"), String::new());
        }
        if joined.ends_with("--help") {
            let help = match (cli, joined.as_str()) {
                ("codex", "exec resume --help") => "--json --model -c, --config",
                _ => scripted_help(cli),
            };
            return (0, help.to_owned(), String::new());
        }
        if joined.contains("upstroke_probe_deliberately_unknown") {
            return (
                2,
                String::new(),
                "error: unknown key `upstroke_probe_deliberately_unknown` in -c override"
                    .to_owned(),
            );
        }
        if joined.contains("model_reasoning_effort=") {
            return (
                2,
                String::new(),
                "error: output schema `upstroke-output-schema-must-not-exist.json` does not exist"
                    .to_owned(),
            );
        }
        if joined == "debug models" {
            let models: Vec<serde_json::Value> = crate::catalog::known_models("codex")
                .into_iter()
                .map(|slug| {
                    serde_json::json!({
                        "slug": slug,
                        "supported_reasoning_levels": Effort::ALL
                            .into_iter()
                            .map(|effort| serde_json::json!({ "effort": effort.to_string() }))
                            .collect::<Vec<_>>(),
                    })
                })
                .collect();
            return (
                0,
                serde_json::json!({ "models": models }).to_string(),
                String::new(),
            );
        }
        (0, String::new(), String::new())
    }

    #[test]
    fn an_adapters_program_is_the_boundarys_and_the_coordinator_host_is_never_asked() {
        let mut host_has = 0_usize;
        let mut host_lacks = 0_usize;
        for adapter in ADAPTERS {
            let name = expected_name(adapter.id());
            let boundary = Boundary::holding(name);

            let spec = adapter
                .build(&a_run(adapter.id()))
                .expect("a named CLI always builds: nothing is resolved");
            assert_eq!(spec.program, name, "{}: built the wrong CLI", adapter.id());
            let program = std::path::Path::new(&spec.program);
            assert!(
                !program.is_absolute(),
                "{}: built `{}`, a location rather than a name",
                adapter.id(),
                spec.program
            );
            assert_eq!(
                program.components().count(),
                1,
                "{}: built `{}`, which carries a directory",
                adapter.id(),
                spec.program
            );

            let caps = adapter
                .probe(&boundary)
                .unwrap_or_else(|error| panic!("{}: {error}", adapter.id()));
            assert_eq!(
                caps.version,
                "9.9.9",
                "{}: certified a version no boundary reported",
                adapter.id()
            );

            let asked = boundary.programs();
            assert!(
                !asked.is_empty(),
                "{}: the boundary was never asked what it has",
                adapter.id()
            );
            assert!(
                asked.iter().all(|program| program == name),
                "{}: a request carried something other than the CLI name: {asked:?}",
                adapter.id()
            );
            assert_eq!(
                boundary.seen()[0].args,
                vec!["--version".to_owned()],
                "{}: the first thing asked of a boundary is not what it has",
                adapter.id()
            );

            match crate::util::find_program(name) {
                Some(resolved) => {
                    assert_ne!(
                        spec.program,
                        resolved.to_string_lossy(),
                        "{}: the program is this host's resolution of the name",
                        adapter.id()
                    );
                    host_has += 1;
                }
                None => host_lacks += 1,
            }
        }
        assert_eq!(
            host_has + host_lacks,
            ADAPTERS.len(),
            "every shipped adapter was measured"
        );
    }

    #[test]
    fn a_cli_this_host_does_not_have_still_certifies_at_the_boundary() {
        let absent: Vec<_> = ADAPTERS
            .iter()
            .filter(|adapter| crate::util::find_program(expected_name(adapter.id())).is_none())
            .collect();
        assert!(
            !absent.is_empty(),
            "every shipped agent CLI is installed on this machine, so \"absent on the host, \
             present at the boundary\" cannot be observed here"
        );

        for adapter in absent {
            let name = expected_name(adapter.id());
            let boundary = Boundary::holding(name);
            let caps = adapter.probe(&boundary).unwrap_or_else(|error| {
                panic!(
                    "{}: a CLI absent from this host was refused before the boundary answered: \
                     {error}",
                    adapter.id()
                )
            });
            assert_eq!(caps.version, "9.9.9");
            assert!(
                adapter.build(&a_run(adapter.id())).is_ok(),
                "{}: a CLI absent from this host could not be built for",
                adapter.id()
            );
        }
    }

    #[test]
    fn a_cli_this_host_does_not_have_is_refused_by_name_and_by_boundary() {
        use crate::runner::host::HostRunner;

        let absent: Vec<_> = ADAPTERS
            .iter()
            .filter(|adapter| crate::util::find_program(expected_name(adapter.id())).is_none())
            .collect();
        assert!(
            !absent.is_empty(),
            "every shipped agent CLI is installed on this machine, so a host-boundary refusal \
             cannot be observed here"
        );

        let runner = HostRunner::new();
        let mut messages = std::collections::BTreeSet::new();
        for adapter in absent {
            let name = expected_name(adapter.id());
            let error = adapter
                .probe(&runner)
                .expect_err("this host has no such CLI, so the boundary cannot certify it");
            let message = error.to_string();
            assert!(
                message.contains(name),
                "{}: the refusal must name the CLI: {message}",
                adapter.id()
            );
            assert!(
                message.contains("installed inside the boundary that executes it"),
                "{}: the refusal must say which boundary is missing it: {message}",
                adapter.id()
            );
            assert!(
                message.contains("on PATH for the host runner"),
                "{}: the refusal must say what to do about it: {message}",
                adapter.id()
            );
            assert!(
                message.contains("no `") && message.contains("` on PATH either"),
                "{}: the refusal must say this host does not have it either: {message}",
                adapter.id()
            );
            assert!(
                message.contains("nothing of that name is on the PATH this runner composes"),
                "{}: the CLI was not refused before the spawn: {message}",
                adapter.id()
            );
            assert!(
                !message.contains("failed to spawn"),
                "{}: the boundary tried to spawn a name it could not resolve: {message}",
                adapter.id()
            );
            messages.insert(message);
        }
        assert_eq!(
            messages.len(),
            ADAPTERS
                .iter()
                .filter(|adapter| crate::util::find_program(expected_name(adapter.id())).is_none())
                .count(),
            "two absent CLIs produced one refusal: {messages:?}"
        );
    }

    #[test]
    fn the_program_preflight_certifies_is_the_program_the_attempt_would_run() {
        for adapter in ADAPTERS {
            let name = expected_name(adapter.id());

            let build_first = Boundary::holding(name);
            let built_first = adapter.build(&a_run(adapter.id())).expect("build");
            adapter.probe(&build_first).expect("probe");

            let probe_first = Boundary::holding(name);
            adapter.probe(&probe_first).expect("probe");
            let built_second = adapter.build(&a_run(adapter.id())).expect("build");

            assert_eq!(
                built_first,
                built_second,
                "{}: the order of build and probe changed what build produces",
                adapter.id()
            );
            assert_eq!(
                build_first.programs(),
                probe_first.programs(),
                "{}: the order changed what pre-flight sent",
                adapter.id()
            );
            for program in build_first.programs() {
                assert_eq!(
                    program,
                    built_first.program,
                    "{}: pre-flight certified `{program}` while the attempt would run `{}`",
                    adapter.id(),
                    built_first.program
                );
            }
        }
    }

    #[test]
    fn two_boundaries_in_one_process_each_certify_their_own_cli() {
        use std::collections::BTreeSet;

        for adapter in ADAPTERS {
            let name = expected_name(adapter.id());
            let mut versions = BTreeSet::new();

            for order in [["1.1.1", "2.2.2"], ["2.2.2", "1.1.1"]] {
                let first = Boundary::holding_version(name, order[0]);
                let second = Boundary::holding_version(name, order[1]);

                let from_first = adapter.probe(&first).expect("first boundary");
                let from_second = adapter.probe(&second).expect("second boundary");

                assert_eq!(
                    from_first.version,
                    order[0],
                    "{}: the first boundary's own version was not what it certified",
                    adapter.id()
                );
                assert_eq!(
                    from_second.version,
                    order[1],
                    "{}: the second boundary in this process was handed the first one's answer",
                    adapter.id()
                );
                assert_eq!(
                    first.programs(),
                    second.programs(),
                    "{}: the two boundaries were asked different things",
                    adapter.id()
                );
                versions.insert(from_first.version);
                versions.insert(from_second.version);
            }
            assert_eq!(
                versions.len(),
                2,
                "{}: the two boundaries did not report two distinct versions, so a shared \
                 answer would be unobservable",
                adapter.id()
            );
        }
    }

    #[test]
    fn the_adapters_hold_no_process_wide_resolution_state() {
        const HOLDERS: [&str; 4] = ["static ", "thread_local!", "OnceLock", "LazyLock"];

        let sources = [
            ("src/agent/bin.rs", include_str!("bin.rs")),
            ("src/agent/claude.rs", include_str!("claude.rs")),
            ("src/agent/codex.rs", include_str!("codex.rs")),
            ("src/agent/copilot.rs", include_str!("copilot.rs")),
        ];

        assert_eq!(sources.len(), ADAPTERS.len() + 1, "each adapter and bin.rs");
        let holders_in = |source: &str| {
            let code = crate::effects::production_code(source);
            HOLDERS
                .into_iter()
                .filter(|holder| {
                    code.match_indices(*holder).any(|(at, _)| {
                        *holder != "static "
                            || code.get(..at).and_then(|before| before.chars().next_back())
                                != Some('\'')
                    })
                })
                .collect::<Vec<_>>()
        };

        let inert = r###"
pub const REVIEW_CANARY_TEXT: &str = "OnceLock";
fn label() -> &'static str { "OnceLock" }
const RAW: &str = r#"static CACHE: LazyLock; #[cfg(test)]"#;
const BYTES: &[u8] = b"thread_local!";
const RAW_BYTES: &[u8] = br#"OnceLock"#;
const CHARACTER: char = 's';
// static COMMENT: OnceLock<LazyLock<()>>;
/* thread_local! { static BLOCK: usize = 0; } */
"###;
        assert!(
            holders_in(inert).is_empty(),
            "literals and comments are inert"
        );
        let test_only = r#"
#[cfg(test)]
mod tests {
    static TEST_CACHE: OnceLock<()> = OnceLock::new();
}
"#;
        assert!(
            holders_in(test_only).is_empty(),
            "test-only state is excluded"
        );
        for (declaration, expected) in [
            ("static CACHE: usize = 0;", vec!["static "]),
            (
                "thread_local! { static CACHE: usize = 0; }",
                vec!["static ", "thread_local!"],
            ),
            ("struct Cache { value: OnceLock<String> }", vec!["OnceLock"]),
            ("struct Cache { value: LazyLock<String> }", vec!["LazyLock"]),
        ] {
            assert_eq!(holders_in(declaration), expected, "{declaration}");
            assert_eq!(
                holders_in(&format!("{inert}\n{test_only}\n{declaration}")),
                expected,
                "production state after inert or test-only text remains visible"
            );
        }

        let mut offenders: Vec<String> = Vec::new();
        for (name, source) in sources {
            for holder in holders_in(source) {
                offenders.push(format!("{name}: {holder}"));
            }
        }
        assert!(
            offenders.is_empty(),
            "an adapter holds a value across calls: {offenders:?}. A resolution remembered in one \
             is handed to the next boundary — `PR4-ADAPTER-RESOLVES-ON-THE-HOST`"
        );
    }

    #[test]
    fn the_host_runner_executes_a_bare_program_name_as_it_executes_the_resolved_path() {
        use crate::runner::host::HostRunner;
        use crate::runner::{InvocationId, gate_request};

        let resolved = crate::util::find_program("git")
            .expect("git is on PATH wherever this repository builds");
        let resolved = resolved
            .to_str()
            .expect("this repository's git lives at a Unicode path")
            .to_owned();
        assert_ne!(resolved, "git", "the two program strings must differ");
        assert!(std::path::Path::new(&resolved).is_absolute());

        let runner = HostRunner::new();
        let run_one = |program: &str, ordinal: u32| {
            let request = gate_request(
                CommandSpec::new(program).arg("--version"),
                PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                Duration::from_secs(60),
                InvocationId::probe(crate::runner::ProbeTarget::Shell, ordinal)
                    .expect("a shell probe identity"),
            );
            runner.run(&request).expect("git --version runs")
        };

        let named = run_one("git", 0);
        let at_path = run_one(&resolved, 1);

        assert_eq!(named.code, Some(0), "{}", named.stderr);
        assert_eq!(
            named.code, at_path.code,
            "the bare name and the resolved path exited differently"
        );
        assert_eq!(
            named.stdout.trim(),
            at_path.stdout.trim(),
            "the bare name and the resolved path are different programs"
        );
        assert!(
            named.stdout.contains("git version"),
            "this did not run git at all: {:?}",
            named.stdout
        );
    }
}

#[cfg(test)]
mod probe_identity_tests {
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn each_agent_probe_request_names_its_own_agent_in_every_field() {
        use crate::runner::{AgentId, CommandSpec, ExecutionRole, ProbeTarget};
        use std::time::Duration;

        fn spec() -> CommandSpec {
            CommandSpec {
                program: "irrelevant".to_owned(),
                args: Vec::new(),
                env: Vec::new(),
                stdin: Vec::new(),
            }
        }

        const NAMES: [&str; 3] = ["claude-code", "codex", "copilot"];
        for order in [
            NAMES.to_vec(),
            NAMES.iter().rev().copied().collect::<Vec<_>>(),
        ] {
            let mut roles = BTreeSet::new();
            let mut agents = BTreeSet::new();
            let mut identities = BTreeSet::new();
            for (index, name) in order.iter().enumerate() {
                let ordinal = u32::try_from(index).expect("small") + 1;
                let request = super::probe_request(name, spec(), ordinal, Duration::from_secs(30))
                    .expect("build a probe request");
                assert_eq!(
                    request.role,
                    ExecutionRole::Probe(ProbeTarget::Agent(AgentId::new(*name))),
                    "the probe role names another agent"
                );
                assert_eq!(
                    request.agent.as_ref().map(AgentId::as_str),
                    Some(*name),
                    "the request's agent is not the one probed"
                );
                let rendered = request.invocation.render();
                assert!(
                    rendered.contains(name),
                    "the invocation identity does not name {name}: {rendered}"
                );
                roles.insert(request.role.label());
                agents.insert(request.agent.map(|agent| agent.as_str().to_owned()));
                identities.insert(rendered);
            }
            assert_eq!(roles.len(), 3, "{roles:?}");
            assert_eq!(agents.len(), 3, "{agents:?}");
            assert_eq!(identities.len(), 3, "{identities:?}");
        }
    }

    fn use_sites(source: &str) -> Vec<String> {
        let mut kept: Vec<String> = Vec::new();
        let mut skipping_to: Option<String> = None;
        let lines: Vec<&str> = source.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            if let Some(closing) = &skipping_to {
                if line == closing.as_str() {
                    skipping_to = None;
                }
                index += 1;
                continue;
            }
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            if trimmed.starts_with("mod probe_ordinal {")
                || (trimmed == "#[cfg(test)]"
                    && lines
                        .get(index + 1)
                        .is_some_and(|next| next.trim_start().starts_with("mod ")))
            {
                skipping_to = Some(format!("{indent}}}"));
                index += 1;
                continue;
            }
            if trimmed.contains("probe_ordinal::")
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("*")
            {
                kept.push(trimmed.to_owned());
            }
            index += 1;
        }
        kept
    }

    #[test]
    fn every_probe_call_site_passes_its_own_ordinal_constant() {
        struct Adapter {
            name: &'static str,
            source: &'static str,
            declared: &'static [&'static str],
            block_parts: &'static [&'static str],
            call_sites: usize,
        }
        let adapters = [
            Adapter {
                name: "claude-code",
                source: include_str!("claude.rs"),
                declared: &["VERSION", "HELP", "AUTH_STATUS"],
                block_parts: &[],
                call_sites: 3,
            },
            Adapter {
                name: "copilot",
                source: include_str!("copilot.rs"),
                declared: &["VERSION", "HELP"],
                block_parts: &[],
                call_sites: 2,
            },
            Adapter {
                name: "codex",
                source: include_str!("codex.rs"),
                declared: &[
                    "VERSION",
                    "EXEC_HELP",
                    "RESUME_HELP",
                    "CONFIG_BASE",
                    "CONFIG_PER_SURFACE",
                    "PROBE_MODELS",
                    "LOGIN_STATUS",
                    "DISCOVER_MODELS",
                ],
                block_parts: &["CONFIG_BASE", "CONFIG_PER_SURFACE"],
                call_sites: 7,
            },
        ];

        let mut total_sites = 0_usize;
        for adapter in adapters {
            let sites = use_sites(adapter.source);
            assert!(
                !sites.is_empty(),
                "{}: no ordinal use site was found, so this test measures nothing",
                adapter.name
            );

            let mut used: BTreeMap<String, usize> = BTreeMap::new();
            for site in &sites {
                let mentioned: Vec<String> = site
                    .match_indices("probe_ordinal::")
                    .map(|(at, _)| {
                        site[at + "probe_ordinal::".len()..]
                            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                            .next()
                            .unwrap_or_default()
                            .to_owned()
                    })
                    .collect();
                for name in &mentioned {
                    assert!(
                        adapter.declared.contains(&name.as_str()),
                        "{}: `{site}` names `{name}`, which the module does not declare",
                        adapter.name
                    );
                    *used.entry(name.clone()).or_default() += 1;
                }
                let bare =
                    mentioned.len() == 1 && *site == format!("probe_ordinal::{},", mentioned[0]);
                assert!(
                    bare || mentioned
                        .iter()
                        .all(|name| adapter.block_parts.contains(&name.as_str())),
                    "{}: `{site}` is neither a bare ordinal argument nor an index into a \
                     documented block — an expression over an ordinal is how two processes \
                     come to share one identity",
                    adapter.name
                );
            }

            let collisions: Vec<(&String, &usize)> =
                used.iter().filter(|(_, count)| **count > 1).collect();
            assert!(
                collisions.is_empty(),
                "{}: {collisions:?} reached the Runner from more than one place, so those \
                 processes share an invocation identity",
                adapter.name
            );

            let declared: BTreeSet<&str> = adapter.declared.iter().copied().collect();
            let actually_used: BTreeSet<&str> = used.keys().map(String::as_str).collect();
            assert_eq!(
                actually_used, declared,
                "{}: the declared ordinals and the ones production uses have diverged",
                adapter.name
            );

            let calls = adapter
                .source
                .lines()
                .filter(|line| {
                    let trimmed = line.trim_start();
                    !trimmed.starts_with("///") && trimmed.contains("probe_request(")
                })
                .count();
            assert_eq!(
                calls, adapter.call_sites,
                "{}: probe call sites moved",
                adapter.name
            );
            total_sites += calls;
        }
        assert_eq!(total_sites, 12, "the adapters' probe call sites moved");
    }
}
