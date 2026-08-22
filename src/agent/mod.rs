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

    /// A stdout every adapter would call a **success**, written in that
    /// adapter's own answer shape.
    ///
    /// Load-bearing rather than convenient: it is what makes the supervision
    /// grid below hostile. With a failure payload, dropping the supervision
    /// checks would still report `AgentError` and the cells would pass for the
    /// wrong reason.
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

    /// **Every** adapter maps **every** supervision result the same way.
    ///
    /// `invariants_preserved[0]` is "process supervision, timeout, output
    /// capture, **adapter parsing** unchanged", and the supervisor's two flags
    /// are inputs to parsing, not to it alone: `output_limited` means the tree
    /// was terminated with the transcript truncated, and `timed_out` means it
    /// was terminated for exceeding its wall clock. A truncated transcript
    /// authorizes nothing (`PR5-CORRECTNESS-013`) and a timeout is a distinct
    /// ladder input from a generic agent failure (`PR5-CORRECTNESS-014`).
    ///
    /// The domain is `ADAPTERS`, from the type, and the expectations are
    /// literals — a table, not a re-derivation of the branch order. That is the
    /// point: the two rows where an exit code of 0 meets a set flag are exactly
    /// the rows a re-derivation would get wrong in the same direction the code
    /// would.
    ///
    /// Claude and Copilot had direct flag-to-status tests; Codex's flag
    /// fixtures exercised its strict-config *preflight validators*, so no test
    /// had ever parsed an output-limited or timed-out Codex execution. That is
    /// the "guarantee proved for the variant that was looked at" class, and the
    /// guard for it is a domain taken from the type.
    #[test]
    fn every_adapter_maps_every_supervision_result_the_same_way() {
        use crate::ir::OutcomeStatus;

        /// One supervision shape: name, exit code, `timed_out`,
        /// `output_limited`, the status every adapter must report, and a
        /// substring the detail must carry.
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
                // The duration is the supervisor's, on every route.
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
        // A `Timeout` really is a different answer from an `AgentError`, which
        // is what makes cells 4 and 5 worth having: the ladder acts on it.
        assert_ne!(OutcomeStatus::Timeout, OutcomeStatus::AgentError);
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
    use std::sync::{Mutex, PoisonError};

    /// A boundary with an agent CLI installation the test invents.
    ///
    /// **This is the test's oracle, and it is deliberately not this machine's
    /// filesystem.** The property being measured — *which* environment an
    /// adapter's program was resolved against — cannot be measured with a
    /// predicate over the same filesystem production consults: `is_file()` is
    /// true of a host-resolved path whether the resolution was right or wrong,
    /// so the oracle blesses either answer (`PR4-CONF-012`). A boundary the
    /// test invents has an installation the test knows about and the host does
    /// not, so "the boundary decided" and "the coordinator host decided" become
    /// different observations.
    ///
    /// It refuses any program that is not the one it has, the way a real
    /// boundary would, and records every request so a boundary that was
    /// **never asked** is distinguishable from one that answered. It also
    /// reports a **version of its own**, because `Caps.version` certifying the
    /// host's CLI while the attempt runs the image's is DESIGN.md:612's
    /// sentence and a distinct failure from either of the other two.
    struct Boundary {
        /// The only program string this boundary will execute.
        installed: String,
        /// What `--version` prints here. Unforgeable by the host: no machine
        /// has an agent CLI at this version.
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
        fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, TactusError> {
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
            Err(TactusError::Agent {
                message: format!(
                    "`{}` is not present inside this boundary; the agent CLI here is `{}`",
                    request.command.program, self.installed
                ),
            })
        }
    }

    /// The bare name each adapter's CLI is installed under, **written here**
    /// rather than read from the adapter's own constant: a name compared only
    /// against the code that produced it proves nothing.
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

    /// The help text each adapter's `--help` contract demands, written here
    /// from the flags each adapter requires rather than captured from a CLI.
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

    /// A boundary that can satisfy a whole pre-flight, answering **by
    /// argument** and never by program.
    ///
    /// Keying an answer on the program string would make the fixture agree
    /// with whatever the adapter sent, which is the self-oracle this whole
    /// file exists to avoid. The one place the program is consulted is
    /// [`Boundary::run`]'s presence check, which is the boundary's own
    /// question — "do I have this?" — and the thing under test.
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
        // Codex's six strict-config parser probes. The two assignments are
        // transcribed here rather than read from that adapter's private
        // constants, so a renamed probe key fails this fixture loudly instead
        // of silently agreeing with itself.
        if joined.contains("tactus_probe_deliberately_unknown") {
            return (
                2,
                String::new(),
                "error: unknown key `tactus_probe_deliberately_unknown` in -c override".to_owned(),
            );
        }
        if joined.contains("model_reasoning_effort=") {
            return (
                2,
                String::new(),
                "error: output schema `tactus-output-schema-must-not-exist.json` does not exist"
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

    /// An adapter's program is the **boundary's**, and the coordinator host is
    /// never asked what it has.
    ///
    /// This replaces `an_adapters_program_is_the_coordinator_hosts_and_the_
    /// boundary_supplies_none`, which pinned PR4's behaviour deliberately and
    /// which a correct PR6 must fail by name. The property that **moved
    /// across** is the old test's claim 2 — "what pre-flight sends and what the
    /// attempt would send are one program", DESIGN.md:612 in the only form PR4
    /// could hold it. It is now held by
    /// [`the_program_preflight_certifies_is_the_program_the_attempt_would_run`]
    /// in both call orders, because the ordering PR4 needed was a property of
    /// the process-wide resolution cache and that cache is gone. The old
    /// claims 1 and 3 are **inverted** here: the boundary is asked, and a CLI
    /// the coordinator host cannot resolve is no longer refused before it is.
    ///
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST`'s three separate failures, and where
    /// each dies:
    ///
    /// 1. *the normal container case is refused before the runtime is asked* —
    ///    every adapter certifies against a boundary the coordinator host does
    ///    not share, and
    ///    [`a_cli_this_host_does_not_have_still_certifies_at_the_boundary`]
    ///    witnesses the absent-on-the-host half on a machine that really lacks
    ///    one;
    /// 2. *every spec carries a path that names nothing at the boundary* — the
    ///    program every request carries is one path component, not absolute,
    ///    and equal to the name written in [`expected_name`]; and
    /// 3. *`Caps.version` certifies the host's CLI while the attempt runs the
    ///    image's* — the version returned is the boundary's invented `9.9.9`,
    ///    which no installation on any machine reports, and
    ///    [`two_boundaries_in_one_process_each_certify_their_own_cli`] holds it
    ///    across two boundaries.
    ///
    /// The second field this holds constant is **what this machine has
    /// installed**: the assertions above are true whichever branch a machine
    /// takes, and both branches are counted so a machine cannot make the test
    /// mean less by having more.
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

            // The discrimination against the old behaviour, on either kind of
            // machine. Where this host has the CLI, the old code would have
            // sent that absolute path and this boundary would have refused it;
            // where it does not, the old code refused before asking at all.
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

    /// A CLI this coordinator host does not have certifies anyway, because the
    /// boundary has it.
    ///
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST` failure (1), witnessed rather than
    /// argued: DESIGN.md:612's normal container case is "an image with
    /// version-pinned CLIs", and until PR6 that image's CLI was refused at
    /// pre-flight because *this* machine had no such file.
    ///
    /// The premise is asserted rather than hoped for: with all three CLIs
    /// installed here the absence half is unobservable, and a silent skip would
    /// measure nothing while looking green. Both machines this slice is
    /// measured on satisfy it — this box has `claude` and `codex` and no
    /// `copilot`, and the Windows guest has none of the three.
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

    /// The other side of that: when the boundary **is** this host and this host
    /// does not have the CLI, the operator is told so, by name and by boundary.
    ///
    /// The fail-closed half of `PR6D-001`'s repair, end to end. The runner's own
    /// refusal is asserted in `runner::host::tests`; this is the sentence the
    /// operator actually reads, which is [`bin::boundary_refused`] wrapping it,
    /// and the two had never been composed. What it must not be is a bare
    /// `NotFound`: before the runner resolved names, an npm-installed CLI on
    /// Windows failed with "program not found" naming no boundary and no
    /// remedy, which is the failure nobody could diagnose from a log.
    ///
    /// The premise — that this machine lacks at least one of the three — is
    /// asserted rather than hoped for, the same way its sibling above asserts
    /// it, so a machine with all three installed reports that it cannot observe
    /// this instead of passing vacuously. Both measured machines satisfy it:
    /// this box has no `copilot`, the Windows guest has none of the three.
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
            // And it is the **resolution's** refusal, not a spawn's. This is
            // the fail-closed clause itself: a runner that handed the name to
            // `Command` anyway would produce a `NotFound` here, which names no
            // boundary and is what an operator cannot act on.
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
        // One distinct sentence per absent CLI: a refusal that named the first
        // adapter for all of them would collapse this to one.
        assert_eq!(
            messages.len(),
            ADAPTERS
                .iter()
                .filter(|adapter| crate::util::find_program(expected_name(adapter.id())).is_none())
                .count(),
            "two absent CLIs produced one refusal: {messages:?}"
        );
    }

    /// Pre-flight certifies the program the attempt would run — in **both call
    /// orders**.
    ///
    /// This is the property that moved here from the test this replaced.
    /// There, `build` had to run **first**, and its comment said why: each
    /// adapter memoised its resolution in a process-wide `OnceLock`, so a probe
    /// reaching an unfilled cache wrote the fixture's answer into it and
    /// changed what every sibling test in the binary resolved. The ordering was
    /// load-bearing because the answer was *state*. It is now a function of its
    /// argument, so the order cannot matter — and asserting both orders is what
    /// says so.
    ///
    /// The second field held constant is the adapter; what varies is the order,
    /// and the two orders must agree exactly.
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

    /// Two boundaries in one process each certify **their own** CLI.
    ///
    /// The hazard this exists for: with two runners in one process — which is
    /// exactly what the container runner introduces — a resolution cached on
    /// nothing hands one boundary's answer to the other, and a value that is
    /// correct on first use and wrong on the second is invisible to any test
    /// that constructs one runner. The repair is that there is no cache; this
    /// is what says so, and it is what fails if one comes back.
    ///
    /// Two independently varying fields, and their intersection is the test:
    /// **which boundary** (two, reporting different versions) x **in which
    /// order** (both). Three distinct versions are asserted as distinct-value
    /// counts rather than described, so a fixture that lost a version reports
    /// it instead of agreeing with itself.
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

    /// No adapter holds process-wide resolution state, and this is the answer
    /// to "what is the cache keyed on".
    ///
    /// **Nothing is cached, so there is no key.** Each adapter used to memoise
    /// its resolved binary in a `static RESOLVED: OnceLock<Option<Invocation>>`
    /// — a cell keyed on nothing, which with two runners in one process hands
    /// one boundary's answer to the other. A resolution that is correct on
    /// first use and wrong on the second is the hardest shape in this slice
    /// precisely because it is invisible to any test that constructs one
    /// runner, and it stays invisible to a *behavioural* test even with two,
    /// because a cache of a constant is indistinguishable from no cache.
    ///
    /// So the claim is structural and the census is the only thing that can
    /// make it: an adapter that starts remembering an answer fails here on the
    /// line that declares the cell, before any behaviour depends on it.
    ///
    /// Comments are stripped, and the strip is asserted to have removed
    /// something — `PR4-CENSUS-COMMENT-ORACLE` is a census over a file format
    /// that has comments, and this file's own prose names every pattern below.
    #[test]
    fn the_adapters_hold_no_process_wide_resolution_state() {
        /// Every way an adapter could hold a value across calls. Written out
        /// rather than derived: a list read from the tree would shrink with it.
        ///
        /// `static ` rather than a list of cell types, and that is deliberate.
        /// Every adapter is a unit struct with no state of its own, so the only
        /// way one remembers anything between calls is a module-level item —
        /// and naming `OnceLock` alone would miss the `static X: Mutex<_>` that
        /// a `const fn` constructor makes just as easy. `OnceLock` and
        /// `LazyLock` are named as well so a `let`-bound or field-held cell is
        /// caught too.
        const HOLDERS: [&str; 4] = ["static ", "thread_local!", "OnceLock", "LazyLock"];

        let sources = [
            ("src/agent/bin.rs", include_str!("bin.rs")),
            ("src/agent/claude.rs", include_str!("claude.rs")),
            ("src/agent/codex.rs", include_str!("codex.rs")),
            ("src/agent/copilot.rs", include_str!("copilot.rs")),
        ];

        let mut stripped = 0_usize;
        let mut offenders: Vec<String> = Vec::new();
        for (name, source) in sources {
            let production = crate::effects::production_region(source);
            let kept: Vec<&str> = production
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect();
            stripped += production.lines().count() - kept.len();
            let code = kept.join("\n");
            for holder in HOLDERS {
                if code.contains(holder) {
                    offenders.push(format!("{name}: {holder}"));
                }
            }
        }
        assert!(
            stripped > 100,
            "the comment strip removed {stripped} lines, so it is not working and this census \
             would be reading prose"
        );
        // The control: the pattern really does match when it is present, so an
        // empty result means absence rather than a broken search.
        assert!(
            HOLDERS
                .into_iter()
                .all(|holder| format!("let x: {holder} = ();").contains(holder)),
            "the census pattern matches nothing at all"
        );
        assert!(
            offenders.is_empty(),
            "an adapter holds a value across calls: {offenders:?}. A resolution remembered in one \
             is handed to the next boundary — `PR4-ADAPTER-RESOLVES-ON-THE-HOST`"
        );
    }

    /// The host runner executes a bare program name as it executes the path
    /// the adapters used to resolve.
    ///
    /// The other direction of this repair, and the one every existing test and
    /// the whole v0.1 product depends on: a change that resolves correctly for
    /// a container boundary and quietly changes what the **host** runner runs is
    /// a defect. The suite cannot show this on its own — it was written against
    /// the old shape and largely still passes — so this spawns the same program
    /// twice through the real [`crate::runner::host::HostRunner`], once named
    /// and once at the absolute path `util::find_program` picks (which is the
    /// resolution the adapters performed), and requires the two to agree.
    ///
    /// `git` rather than an agent CLI because every machine that can build this
    /// repository has it, so the observation is not a property of what happens
    /// to be installed. The two program strings are asserted to actually differ
    /// first; if they did not, this would compare a thing with itself.
    ///
    /// **What this row cannot express, and where that lives instead.** `git` is
    /// a native `.exe`, and on Windows `CreateProcessW` appends `.exe` to a bare
    /// name, so the bare spelling reaches it whether or not this runner resolves
    /// anything — which is exactly the installation shape `PR6D-001` did *not*
    /// break. As a witness for the Windows case this row is therefore
    /// correlated: it was green while the defect was live. It is kept as the
    /// control it can honestly be — a real program on this machine's real
    /// `PATH`, which the repair must not have changed — and the axis it cannot
    /// vary, an npm-style `.cmd`-only installation reachable only through
    /// `PATHEXT`, is
    /// `runner::host::tests::an_npm_style_installation_runs_by_bare_name_exactly_as_it_runs_by_path`.
    /// That test lives beside the resolution because writing a shim needs the
    /// effect allowance `effects/allowlist.toml` gives `src/runner/host.rs` and
    /// does not give this module.
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
    /// non-bare uses are the one block codex documents — a base plus an index,
    /// for its six strict-config parser probes.
    ///
    /// Codex had a second such block until PR6, one process per PATH
    /// candidate, because it resolved its binary by spawning each candidate on
    /// the coordinator host. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` removed the
    /// resolution, so `RESOLUTION_BASE` and its call site are gone and the
    /// counts below drop by one.
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
                ],
                // The one variable-length step: six strict-config parser
                // probes (two surfaces x three assignments). `probe_ordinal`
                // documents it as a block precisely because it cannot be one
                // constant.
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
        // Hostility as a count: 3 + 2 + 7 across the three adapters, written
        // from what each pre-flight does. Codex was 8 until PR6 removed the
        // per-PATH-candidate resolution spawn
        // (`PR4-ADAPTER-RESOLVES-ON-THE-HOST`).
        assert_eq!(total_sites, 12, "the adapters' probe call sites moved");
    }
}
