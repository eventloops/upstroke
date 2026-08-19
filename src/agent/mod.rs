//! Agent adapters (DESIGN.md §8, §16): turn a `TaskRun` into a subprocess of
//! an official agent CLI and parse what came back. Adapters never edit files,
//! never commit, and never speak HTTP — they only build commands and read
//! process output. One file per agent.

pub mod bin;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod proc;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capacity::PoolKind;
use crate::error::TactusError;
use crate::ir::{Effort, Outcome, WorkerProfile};
use crate::runner::invocation::InvocationId;
use crate::runner::{AgentId, CommandSpec, ExecutionRole, ProbeTarget, Runner, RunnerRequest};

pub use proc::ProcessOutput;

/// Whether the vendor's CLI says it is signed in.
///
/// Three states, not two. "Could not tell" must never render as "not
/// connected": `tactus connect` writes a file an operator then trusts, and a
/// confident *wrong* "you are not logged in" sends them to re-authenticate an
/// account that was fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    Authenticated,
    NotAuthenticated,
    Unknown,
}

/// One rendering, used by `connect` and `capacity` alike.
///
/// There were two: a terse `Display` here and a fuller `describe_auth` in
/// `connect`, so the same fact read as "not authenticated" from one command and
/// "NOT signed in — log in with the vendor's own CLI before running" from the
/// other, and an operator comparing them could not tell whether they described
/// the same thing. The rule this enum exists to enforce — "could not tell"
/// never renders as "not connected" — was then enforced in one place and merely
/// observed in the other.
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

/// What one agent's CLI could be got to say about itself, without the network
/// and without touching a credential (invariants 2 and 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub auth: AuthState,
    /// The models the CLI itself advertises.
    ///
    /// Empty on Claude Code and Copilot today: as of Aug 2026 neither offers
    /// non-interactive model enumeration. Codex exposes its local roster via
    /// `debug models`; its adapter validates model × effort support at probe
    /// and reports the slugs here. The seam lets every real listing be
    /// cross-checked against the shipped catalog; [`Caps::model_list`] is the
    /// gate.
    pub models: Vec<String>,
    /// §13's pool-kind hint, read from whatever the CLI says about the account
    /// it is signed into. `None` means it said nothing conclusive, and the
    /// caller picks a documented default rather than guessing.
    pub shape: Option<PoolKind>,
    /// Everything the operator should know about how this was worked out —
    /// including what could not be.
    pub notes: Vec<String>,
}

impl Discovery {
    /// What an adapter that does not implement discovery reports: nothing,
    /// said out loud.
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

/// Capabilities discovered by `probe()` at pre-flight (§14). Copilot's CLI
/// has shipped breaking flag removals, so capability probing is load-bearing,
/// not decorative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caps {
    /// Version string as reported by the binary, best-effort.
    pub version: String,
    pub json_output: bool,
    pub session_resume: bool,
    pub cost_reporting: bool,
    pub read_only_mode: bool,
    pub acp: bool,
    pub model_list: bool,
}

/// Everything an adapter needs to build one attempt's subprocess. The engine
/// materializes the prompt (§14: body + acceptance + artifacts + conventions
/// brief) — adapters never re-derive it.
#[derive(Debug, Clone)]
pub struct TaskRun {
    /// Fully materialized prompt, delivered on stdin.
    pub prompt: String,
    pub profile: WorkerProfile,
    /// Working directory for the subprocess (the workspace repo root).
    pub workspace: PathBuf,
    /// The gate commands this profile may run, and nothing else (§20). Empty
    /// for reviewers, which run nothing at all.
    ///
    /// Carried on the run rather than only handed to
    /// [`AgentAdapter::materialize_permissions`] because not every agent has a
    /// settings file to put them in: Copilot's permission surface is argv, so
    /// its `build` needs them at command-construction time.
    pub gate_cmds: Vec<String>,
    /// Same-rung retry: resume this session with feedback instead of starting
    /// fresh (§11.4).
    pub resume_session: Option<String>,
    /// Per-run permission settings file, materialized by the engine from
    /// [`claude::permission_settings`]-style generators (§20).
    pub settings_path: Option<PathBuf>,
}

/// Where a pre-flight process runs.
///
/// The coordinator's own working directory, which is exactly what a probe
/// inherited before probes went through the Runner — a probe asks a CLI about
/// itself and has no workspace of its own. Absolute rather than `"."` because
/// `runner::host::HostRunner::run` clears the environment, and on Windows the
/// `=X:` drive-relative variables go with it; every process it starts is given
/// an absolute directory so none of them can be resolving a drive-relative
/// path.
#[must_use]
pub fn probe_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// One pre-flight process of `agent`, as a [`RunnerRequest`].
///
/// `decisions.pr_sequence[5].scope`: "**probes**, workers, gates, reviews go
/// through the Runner", and INV-18 accounts an agent probe the way it accounts
/// an attempt — "every agent CLI invocation **incl. agent probes** acquires
/// its atomic {agent, pool?} pair" — so the role is `probe(<agent>)`, it is
/// slotted, and `agent` is set so `host-v1` supplies that agent's credential
/// location (a probe that could not see the credential directory would certify
/// a CLI in a state the attempt never runs in).
///
/// `ordinal` is **which of this adapter's pre-flight processes this is**. A
/// pre-flight that runs `--version` and then `--help` runs two processes, and
/// "unique per process" is the packet's property, so each adapter fixes a
/// named ordinal per step rather than counting: a counter would renumber every
/// later step the first time an earlier one was skipped (codex's binary
/// resolution caches, so its second call skips one), and the identities of one
/// machine's pre-flight would stop being a function of the pre-flight.
///
/// # Errors
///
/// [`TactusError::Refused`] when the adapter id cannot appear in an invocation
/// identity — see [`InvocationId::probe`]. Every shipped id is `[a-z-]`.
pub fn probe_request(
    agent: &str,
    command: CommandSpec,
    ordinal: u32,
    timeout: Duration,
) -> Result<RunnerRequest, TactusError> {
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

/// DESIGN.md §8 `AgentAdapter`.
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    /// Locate the binary and report version + capabilities. Ran at pre-flight;
    /// a missing binary is a refusal to start, not a task failure (§19).
    ///
    /// Takes the runner because DESIGN.md:209 does — `probe(&self, runner:
    /// &dyn Runner)`, annotated "probes the boundary that will execute" — and
    /// DESIGN.md:612 says why: "Probes run through that same runner, or
    /// pre-flight could certify a host CLI/version different from the one the
    /// attempt executes."
    ///
    /// # Errors
    ///
    /// A missing or unusable binary, a CLI that has dropped a required flag,
    /// or a runner refusal.
    fn probe(&self, runner: &dyn Runner) -> Result<Caps, TactusError>;
    /// Turn one attempt into a **data-only** [`CommandSpec`].
    ///
    /// DESIGN.md:117: an adapter "does not decide where the process runs". A
    /// `build` that returned a live `std::process::Command` could carry a cwd,
    /// an environment, or a spawn past the runner, and PR6's container runner
    /// would inherit the hole — so what comes back is a value with a program,
    /// arguments, an environment **overlay** and stdin bytes, and nothing that
    /// names a machine.
    ///
    /// # Errors
    ///
    /// A refusal to run this profile at all (§19/§20), or a binary that cannot
    /// be located.
    fn build(&self, run: &TaskRun) -> Result<CommandSpec, TactusError>;
    /// Read one attempt's process output as an [`Outcome`].
    ///
    /// # Errors
    ///
    /// Output this adapter cannot interpret at all.
    fn parse(&self, out: &ProcessOutput) -> Result<Outcome, TactusError>;

    /// §13's `tactus connect`: ask this agent's CLI about the account behind
    /// it — signed in or not, what shape its quota is, which models it offers.
    ///
    /// Subprocesses the vendor's own CLI and parses what came back. No HTTP, no
    /// token ever handled, no credential file read: a vendor CLI talking to its
    /// own vendor is the design (invariant 2), the same posture §9 sets for
    /// plan importers.
    ///
    /// Takes the `Caps` the caller already probed rather than re-probing:
    /// discovery always runs beside a probe (a CLI that cannot report its own
    /// version is in no state to be asked about its account), and an adapter
    /// that called `probe()` again spawned `--version` and `--help` a second
    /// time — four subprocesses where two would do, each carrying the probe
    /// timeout.
    ///
    /// The default reports nothing rather than being required, so an adapter
    /// cannot silently claim discovery it does not do — [`Discovery::unknown`]
    /// is an honest "could not tell", and every consumer treats it as one.
    ///
    /// # Errors
    ///
    /// Whatever asking this CLI about its account failed with. The default
    /// never fails: it asks nothing.
    fn discover(&self, _runner: &dyn Runner, _caps: &Caps) -> Result<Discovery, TactusError> {
        Ok(Discovery::unknown())
    }

    /// What to write to the child's stdin. Delivery is the adapter's call:
    /// CLIs that take the prompt as an argument instead return empty here.
    fn stdin_payload<'a>(&self, run: &'a TaskRun) -> &'a str {
        &run.prompt
    }

    /// Materialize this agent's permission surface (§20) into `dir`, returning
    /// the file the command should reference. Claude Code writes a settings
    /// JSON; Copilot will encode permissions as argv flags and write nothing.
    fn materialize_permissions(
        &self,
        _profile: &WorkerProfile,
        _gate_cmds: &[String],
        _dir: &std::path::Path,
        _stem: &str,
    ) -> Result<Option<PathBuf>, TactusError> {
        Ok(None)
    }
}

/// Where a caller finds agent adapters. Injectable so the engine, `connect`
/// and `capacity` are all fully testable without any real agent CLI on the
/// machine.
///
/// Lives here rather than in `engine` because resolving an adapter id has
/// nothing to do with running a plan: `capacity` documents itself as a pure
/// estimator over plain values, and `connect` executes nothing at all, yet both
/// had to import the execution engine for this two-line trait.
pub trait AdapterSource {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter>;
}

pub struct BuiltinAdapters;

impl AdapterSource for BuiltinAdapters {
    fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
        by_id(id).map(|a| a as &dyn AgentAdapter)
    }
}

/// Registry in routing order; ids match `WorkerProfile.agent`.
pub static ADAPTERS: &[&dyn AgentAdapter] = &[
    &claude::ClaudeCodeAdapter,
    &copilot::CopilotAdapter,
    &codex::CodexAdapter,
];

pub fn by_id(id: &str) -> Option<&'static dyn AgentAdapter> {
    ADAPTERS.iter().copied().find(|a| a.id() == id)
}

/// Shared effort levels the help entry for `--effort` actually advertises.
///
/// Looking only for the flag proves too little: several CLI versions exposed
/// `--effort` with a narrower enum. The option's own wrapped help block is
/// parsed so unrelated words elsewhere in `--help` cannot masquerade as a
/// supported value.
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

/// Whether help advertises `flag` as a whole option token.
///
/// Short flags need this instead of substring search: `-p` occurs inside
/// `--permission-mode`, and `-s` inside several unrelated long options.
pub(crate) fn advertises_flag(help: &str, flag: &str) -> bool {
    help.split(|character: char| character.is_whitespace() || character == ',')
        .map(|token| token.split(['=', ':']).next().unwrap_or(token))
        .any(|name| name == flag)
}

/// Rate-limit signals are ground truth for the capacity engine (§13), so both
/// adapters read from one vocabulary rather than two that drift apart.
///
/// Phrases cover the subscription-window wording Claude Code prints ("5-hour
/// limit reached", "Weekly limit reached"), Copilot's credit and premium-request
/// wording (§13's two billing shapes), and API-level errors underneath either.
///
/// Only ever consulted for a FAILED attempt: a successful task *about* rate
/// limiting ("added backoff for 429 responses") must never be read as the pool
/// being exhausted, or verified work gets rolled back.
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

    /// What an agent probe *is*, against the two passages that say so.
    ///
    /// INV-18: "every agent CLI invocation **incl. agent probes** acquires its
    /// atomic {agent, pool?} pair while gates and the shell probe register
    /// without slots" — so it is slotted and it names its agent.
    /// `decisions.admission_and_leases.permits.invocation_identity`: the third
    /// form is "(probe, target: Agent(name) | Shell, ordinal) at pre-flight",
    /// and the shell probe is the *other* target — so an agent probe's
    /// identity names the agent, never `shell`.
    ///
    /// The expected values are written from those sentences, not read back
    /// from the request under test.
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

        // The role the request carries and the target its identity carries are
        // the same agent, and neither is the shell probe's. A request whose
        // identity said `shell` would be a non-slotted process wearing a
        // slotted role.
        assert_ne!(request.invocation.render(), "p.shell.o0");
        assert!(
            !ExecutionRole::Probe(ProbeTarget::Shell).is_slotted(),
            "and the shell probe, which this is not, is the non-slotted one"
        );

        // Every shipped adapter id can be one, and the three are three
        // distinct identities at the same ordinal — the target is a field, not
        // decoration.
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

        // And one adapter's successive pre-flight processes are successive
        // identities, which is what makes "unique per process" hold for a
        // probe that runs `--version` and then `--help`.
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

    /// An id that could not survive a container name is refused rather than
    /// carried. `decisions.pr_sequence[7].scope` puts an invocation id inside
    /// `<R>/containers/<name>.intent`, and `.` is the identity's own field
    /// separator.
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

    /// A built command names a program **this machine resolved**, never a
    /// literal path an adapter carries.
    ///
    /// DESIGN.md:117 — an adapter "does not decide where the process runs" —
    /// and DESIGN.md:612 — "the runner decides where it executes; adapters
    /// never learn about containers". An adapter is still what knows *which
    /// program* to run, and it answers by resolving the CLI on `PATH`; what it
    /// may not do is invent one. Nothing else in the suite executes a
    /// **built-in** adapter's `build` output at all — the parity helper
    /// supplies its own shell spec — so an adapter that returned a hard-coded
    /// program would have left every test green.
    ///
    /// On a machine where a CLI is absent, `build` refuses instead, and the
    /// refusal is asserted too: that is the branch a machine without the CLI
    /// takes, and a test that silently skipped there would be measuring
    /// nothing while looking green.
    #[test]
    fn a_built_command_names_a_program_this_machine_resolved() {
        let profile = |agent: &str| WorkerProfile {
            name: "impl-mid".to_owned(),
            agent: agent.to_owned(),
            model: "a-model".to_owned(),
            pool: "a-pool".to_owned(),
            permissions: PermissionMode::ReadOnly,
            effort: Some(Effort::Medium),
            max_turns: Some(30),
            extra_args: Vec::new(),
        };

        let mut resolved = 0_usize;
        let mut refused = 0_usize;
        for adapter in ADAPTERS {
            let run = TaskRun {
                prompt: "Do the thing.".to_owned(),
                profile: profile(adapter.id()),
                workspace: PathBuf::from("."),
                gate_cmds: Vec::new(),
                resume_session: None,
                settings_path: None,
            };
            // The name this adapter's CLI is installed under, written here
            // rather than read from the adapter's private candidate list.
            let expected_stem = match adapter.id() {
                "claude-code" => "claude",
                "copilot" => "copilot",
                "codex" => "codex",
                other => panic!("an adapter shipped without a name in this table: {other}"),
            };
            match adapter.build(&run) {
                Ok(spec) => {
                    let program = std::path::Path::new(&spec.program);
                    assert!(
                        program.is_file(),
                        "{}: built `{}`, which is not a file on this machine — an adapter's \
                         program is a path it resolved, not one it carries",
                        adapter.id(),
                        spec.program
                    );
                    let stem = program
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    assert_eq!(
                        stem,
                        expected_stem,
                        "{}: built `{}`, which is not this adapter's CLI",
                        adapter.id(),
                        spec.program
                    );
                    resolved += 1;
                }
                Err(error) => {
                    let message = error.to_string();
                    assert!(
                        message.contains(expected_stem),
                        "{}: a build that cannot resolve its CLI must name it: {message}",
                        adapter.id()
                    );
                    refused += 1;
                }
            }
        }
        assert_eq!(
            resolved + refused,
            ADAPTERS.len(),
            "every shipped adapter was asked to build"
        );
    }
}

#[cfg(test)]
mod probe_identity_tests {
    use std::collections::{BTreeMap, BTreeSet};

    /// Each agent probe names **its own** agent, in every field that names one.
    ///
    /// `invariants_introduced[1]` — "RunnerRequest carries a typed
    /// InvocationId (… probes included; the probe role carries target
    /// `Agent(name)` | `Shell`)". Every probe fixture in this suite probes one
    /// agent, so a `probe_request` that filled the target with the first
    /// configured adapter's name would agree with itself on every one of them.
    /// Two independently named probes, and each request checked against the
    /// name it was asked for rather than against the other request.
    ///
    /// Both iteration orders, because "the first configured agent" is a
    /// property of order: a fixture that only ever built them in one order
    /// would pass for the agent that happened to be first.
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

        // Written here, not read from the adapter registry: the names are the
        // expected values.
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
            // Hostility as counts: three names in, three distinct values out of
            // each field that carries one.
            assert_eq!(roles.len(), 3, "{roles:?}");
            assert_eq!(agents.len(), 3, "{agents:?}");
            assert_eq!(identities.len(), 3, "{identities:?}");
        }
    }

    /// The file minus its `#[cfg(test)] mod tests { … }` block and minus the
    /// `mod probe_ordinal { … }` declaration, so what is left is the
    /// production code that *uses* an ordinal.
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
            // Prose mentions an ordinal too, and a comment starts no
            // process.
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

    /// Every ordinal an adapter's pre-flight passes to the Runner, **read
    /// from the call sites** rather than from the table beside them.
    ///
    /// `decisions.admission_and_leases.permits.invocation_identity`: an
    /// invocation identity is "unique **per process**", and "every
    /// RunnerRequest carries it". Each adapter's
    /// `every_preflight_process_has_its_own_ordinal` builds its set from the
    /// `probe_ordinal::ALL` array, so what it asserts is that a *table* has
    /// distinct entries — which stays true when a call site passes another
    /// entry's constant (codex's `debug models` step passing `VERSION`) or an
    /// arithmetic expression over one (`HELP.saturating_sub(1)`). Two
    /// processes then carry `p.agent-<name>.o0` and the ledger cannot tell
    /// them apart, which is exactly what "unique per process" forbids.
    ///
    /// This asserts the property one step later, at the point of use: each
    /// declared constant is used **once**, every one is used, and the only
    /// non-bare uses are the two blocks codex documents — a base plus an
    /// index, for its two variable-length steps.
    #[test]
    fn every_probe_call_site_passes_its_own_ordinal_constant() {
        struct Adapter {
            name: &'static str,
            source: &'static str,
            /// The constants the module declares, written out here from the
            /// steps the adapter performs rather than read from the module.
            declared: &'static [&'static str],
            /// Constants that may appear in an expression, with the block they
            /// open. Everything else must reach the Runner as a bare
            /// `probe_ordinal::NAME` argument.
            block_parts: &'static [&'static str],
            /// How many processes this adapter starts through
            /// `probe_request`, counting each variable-length block as one
            /// call site.
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
                    "RESOLUTION_BASE",
                ],
                // The two variable-length steps: six strict-config parser
                // probes (two surfaces x three assignments) and one process
                // per PATH candidate. `probe_ordinal` documents both as
                // blocks precisely because neither can be one constant.
                block_parts: &["CONFIG_BASE", "CONFIG_PER_SURFACE", "RESOLUTION_BASE"],
                call_sites: 8,
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

            // No constant is used twice: that is the collision this exists to
            // catch, and it is what a table-only test cannot see.
            let collisions: Vec<(&String, &usize)> =
                used.iter().filter(|(_, count)| **count > 1).collect();
            assert!(
                collisions.is_empty(),
                "{}: {collisions:?} reached the Runner from more than one place, so those \
                 processes share an invocation identity",
                adapter.name
            );

            // And every declared constant is used: an ordinal declared and
            // never passed is a step whose identity came from somewhere else.
            let declared: BTreeSet<&str> = adapter.declared.iter().copied().collect();
            let actually_used: BTreeSet<&str> = used.keys().map(String::as_str).collect();
            assert_eq!(
                actually_used, declared,
                "{}: the declared ordinals and the ones production uses have diverged",
                adapter.name
            );

            // The number of processes, counted from the call sites rather
            // than from the table.
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
        // Hostility as a count: 3 + 2 + 8 across the three adapters, written
        // from what each pre-flight does.
        assert_eq!(total_sites, 13, "the adapters' probe call sites moved");
    }
}
