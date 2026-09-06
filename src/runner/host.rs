//! Extended notes: `docs/internals/runner/host.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};

use crate::agent::proc::{self, NoHooks, SpawnHooks};
use crate::agent::{ProcessOutput, claude, codex, copilot};
use crate::error::UpstrokeError;
use crate::gates::ShellKind;
use crate::runner::invocation::InvocationId;
use crate::runner::policy::{host_policy, runner_policy_sha256};
use crate::runner::{AgentId, CommandSpec, ExecutionRole, ProbeTarget, Runner, RunnerRequest};
use crate::topology::effects::ProcessSite;
use crate::topology::events::RunnerPolicy;

mod probe;
pub use self::probe::{SHELL_PROBE_COMMAND, SHELL_PROBE_TIMEOUT, run_shell_probe};

mod environment;
pub use self::environment::{HostEnvironment, KeyCase};

pub const RESERVED_ALWAYS: &[&str] = &["PATH", "HOME", "USERPROFILE"];

pub const CREDENTIAL_LOCATIONS: &[(&str, &str)] = &[
    (claude::ADAPTER_ID, "CLAUDE_CONFIG_DIR"),
    (copilot::ADAPTER_ID, "COPILOT_HOME"),
    (codex::ADAPTER_ID, "CODEX_HOME"),
];

#[must_use]
pub fn reserved_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = RESERVED_ALWAYS.to_vec();
    keys.extend(CREDENTIAL_LOCATIONS.iter().map(|(_, key)| *key));
    keys
}

#[must_use]
pub fn credential_location(agent: &AgentId) -> Option<&'static str> {
    CREDENTIAL_LOCATIONS
        .iter()
        .find(|(id, _)| *id == agent.as_str())
        .map(|(_, key)| *key)
}

const fn supplies_credentials(role: &ExecutionRole) -> bool {
    match role {
        ExecutionRole::Implement
        | ExecutionRole::Review
        | ExecutionRole::Probe(ProbeTarget::Agent(_)) => true,
        ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => false,
    }
}

// This runner owns both locks. `resolved` serializes each program lookup and
// caches its success or error before releasing the guard. Concurrent callers
// reuse that result. It is released before `hooks` is acquired; the locks never
// nest. `hooks` gives one caller exclusive access to the mutable observer during
// startup or an entire supervised run, so callers on one runner wait their turn.
// Guards release on return or unwind. Poisoned locks retain their inner state;
// child cleanup during a run belongs to the process funnel's RAII owners.
pub struct HostRunner {
    policy: RunnerPolicy,
    digest: String,
    environment: HostEnvironment,
    hooks: Mutex<Box<dyn SpawnHooks + Send>>,
    resolved: Mutex<BTreeMap<ProgramQuestion, Result<PathBuf, String>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProgramQuestion {
    program: String,
    path: Option<OsString>,
    pathext: Option<OsString>,
}

impl std::fmt::Debug for HostRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostRunner")
            .field("policy", &self.policy)
            .field("digest", &self.digest)
            .field("environment", &self.environment)
            .finish_non_exhaustive()
    }
}

impl Default for HostRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl HostRunner {
    #[must_use]
    pub fn new() -> Self {
        let policy = host_policy();
        let digest = runner_policy_sha256(&policy);
        Self {
            policy,
            digest,
            environment: HostEnvironment::from_process(),
            hooks: Mutex::new(Box::new(NoHooks)),
            resolved: Mutex::new(BTreeMap::new()),
        }
    }

    #[must_use]
    pub fn with_environment(mut self, environment: HostEnvironment) -> Self {
        self.environment = environment;
        self
    }

    #[must_use]
    pub fn with_hooks(self, hooks: Box<dyn SpawnHooks + Send>) -> Self {
        Self {
            hooks: Mutex::new(hooks),
            ..self
        }
    }

    #[must_use]
    pub const fn policy(&self) -> &RunnerPolicy {
        &self.policy
    }

    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.digest
    }

    #[must_use]
    pub const fn environment(&self) -> &HostEnvironment {
        &self.environment
    }

    pub fn start_write_command(&self) -> Result<Contained, UpstrokeError> {
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
        contain_write_command(&mut **hooks)
    }

    pub fn shell_probe(
        &self,
        shell: ShellKind,
        workspace: &Path,
        invocation: InvocationId,
    ) -> Result<(), UpstrokeError> {
        run_shell_probe(self, shell, workspace.to_path_buf(), invocation)
    }

    fn program_for(
        &self,
        program: &str,
        composed: &[(OsString, OsString)],
    ) -> Result<PathBuf, UpstrokeError> {
        RESOLUTIONS.with(|count| count.set(count.get() + 1));
        let case = self.environment.case();
        let question = ProgramQuestion {
            program: program.to_owned(),
            path: composed_value(composed, case, "PATH").map(OsStr::to_os_string),
            pathext: composed_value(composed, case, "PATHEXT").map(OsStr::to_os_string),
        };
        let mut resolved = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(answer) = resolved.get(&question) {
            return answer
                .clone()
                .map_err(|message| UpstrokeError::Refused { message });
        }
        let answer = resolve_program(program, composed, case, ProgramNaming::current());
        match &answer {
            Ok(file) => {
                resolved.insert(question, Ok(file.clone()));
            }
            Err(UpstrokeError::Refused { message }) => {
                resolved.insert(question, Err(message.clone()));
            }
            Err(_) => {
                // An undetermined answer (for example a stat failure other than
                // not-found) is not the filesystem's, so it is not memoised: the
                // typed error reaches this caller unflattened, and a later call
                // searches again instead of replaying a possibly-transient failure.
            }
        }
        answer
    }
}

impl Runner for HostRunner {
    fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError> {
        let composed = self.environment.compose(
            &request.role,
            request.agent.as_ref(),
            &request.command.env,
        )?;
        let program = self.program_for(&request.command.program, &composed)?;
        let mut command = build_command_at(&request.command, &program);
        command.current_dir(&request.workspace);
        command.env_clear();
        command.envs(composed);
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
        proc::run_with_timeout_at(
            ProcessSite::Spawn,
            ProcessSite::Terminate,
            command,
            &request.command.stdin,
            request.timeout,
            &mut **hooks,
        )
    }
}

fn build_command_at(spec: &CommandSpec, program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    if let Some(switch) = cmd_switch_index(program, spec) {
        use std::os::windows::process::CommandExt;

        for arg in &spec.args[..=switch] {
            command.arg(arg);
        }
        let tail = spec.args[switch + 1..].join(" ");
        if !tail.is_empty() {
            command.raw_arg(tail);
        }
        return command;
    }
    command.args(&spec.args);
    command
}

#[cfg(windows)]
fn cmd_switch_index(program: &Path, spec: &CommandSpec) -> Option<usize> {
    let stem = program
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !stem.eq_ignore_ascii_case("cmd") {
        return None;
    }
    spec.args
        .iter()
        .position(|arg| arg.eq_ignore_ascii_case("/c") || arg.eq_ignore_ascii_case("/k"))
}

mod naming;
use self::naming::{ProgramNaming, composed_value, resolve_program};

thread_local! {
    static RESOLUTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static SEARCHES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[must_use]
pub fn program_resolutions() -> u64 {
    RESOLUTIONS.with(std::cell::Cell::get)
}

#[must_use]
pub fn program_searches() -> u64 {
    SEARCHES.with(std::cell::Cell::get)
}

mod proof {
    use super::ESTABLISHMENTS;
    use crate::agent::proc::{self, SpawnHooks};
    use crate::error::UpstrokeError;

    #[derive(Debug)]
    pub struct Contained(());

    impl Contained {
        fn new() -> Self {
            ESTABLISHMENTS.with(|count| count.set(count.get() + 1));
            Self(())
        }
    }

    pub fn contain_write_command(hooks: &mut dyn SpawnHooks) -> Result<Contained, UpstrokeError> {
        proc::join_ambient_job(hooks).map(|()| Contained::new())
    }
}

pub use self::proof::{Contained, contain_write_command};

thread_local! {
    static ESTABLISHMENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[must_use]
pub fn containment_establishments() -> u64 {
    ESTABLISHMENTS.with(std::cell::Cell::get)
}

pub fn start_write_command(hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {
    contain_write_command(hooks).map(|_contained| ())
}

#[must_use]
pub fn shell_probe_request(
    shell: ShellKind,
    workspace: PathBuf,
    invocation: InvocationId,
) -> RunnerRequest {
    RunnerRequest {
        command: shell.spec(SHELL_PROBE_COMMAND),
        workspace,
        role: ExecutionRole::Probe(ProbeTarget::Shell),
        timeout: SHELL_PROBE_TIMEOUT,
        agent: None,
        invocation,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn build_command(spec: &CommandSpec) -> Command {
        build_command_at(spec, Path::new(&spec.program))
    }
}

#[cfg(test)]
mod tests;
