//! The host runner: `host-v1`.
//!
//! Everything DESIGN.md:118 gives a runner — "cwd, mounts, environment,
//! supervision, and timeout" — for the boundary that is this machine. It wraps
//! the process funnel in [`crate::agent::proc`] rather than reimplementing it.
//!
//! Three things live here that are not in the funnel:
//!
//! * **Environment composition** (DESIGN.md:258-264). The Upstroke environment is
//!   the base; the runner supplies the reserved keys; `CommandSpec.env` is an
//!   overlay applied last and refused pre-flight if it names a reserved key.
//! * **The `RunnerPreflight` shell probe** (INV-23). The recorded shell
//!   executing `exit 0` **through the Runner**, role `probe(shell)`,
//!   non-slotted, a registered invocation.
//! * **The write-command startup step** (INV-18). On Windows the coordinator
//!   joins its ambient kill-on-close Job Object before any spawn; a failure
//!   refuses the write command with a diagnostic.
//!
//! Extended notes: `docs/internals/runner/host.md`
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
// carries this module's review clause -- effects only inside site-taking APIs,
// no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).
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

// ---------------------------------------------------------------------------
// Environment composition
// ---------------------------------------------------------------------------

mod environment;
pub use self::environment::{HostEnvironment, KeyCase};

/// The environment keys `host-v1` owns.
///
/// DESIGN.md:260-262 is the authority; the quote is in
/// `docs/internals/runner/host.md`.
///
/// `USERPROFILE` is here beside `HOME` because on Windows it *is* the home
/// variable — [`crate::util::user_upstroke_dir`] reads it first there and falls
/// back to `HOME`, precisely because Git Bash sets `HOME` to an MSYS path the
/// Windows file APIs cannot open. Reserving one and not the other would let an
/// adapter move the home directory on one platform through the name the other
/// platform trusts.
pub const RESERVED_ALWAYS: &[&str] = &["PATH", "HOME", "USERPROFILE"];

/// Each agent's credential *location* variable — a config **directory**, never
/// a token.
///
/// Each is the vendor's own profile mechanism; `docs/internals/runner/host.md`
/// records which, and from where.
///
/// They are reserved for **every** request rather than only for the agent a
/// request binds: the narrower rule would let a gate — repository-controlled
/// code, the one thing on the host that no agent permission surface bounds —
/// point another agent's CLI at a directory of its choosing.
pub const CREDENTIAL_LOCATIONS: &[(&str, &str)] = &[
    (claude::ADAPTER_ID, "CLAUDE_CONFIG_DIR"),
    (copilot::ADAPTER_ID, "COPILOT_HOME"),
    (codex::ADAPTER_ID, "CODEX_HOME"),
];

/// Every key an overlay may not name.
#[must_use]
pub fn reserved_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = RESERVED_ALWAYS.to_vec();
    keys.extend(CREDENTIAL_LOCATIONS.iter().map(|(_, key)| *key));
    keys
}

/// The credential-location variable of one agent, if the host knows one.
#[must_use]
pub fn credential_location(agent: &AgentId) -> Option<&'static str> {
    CREDENTIAL_LOCATIONS
        .iter()
        .find(|(id, _)| *id == agent.as_str())
        .map(|(_, key)| *key)
}

/// Whether `host-v1` tells this role where an agent's credentials live.
///
/// The one thing the host has to scope by role, and the reason the packet's
/// word is "role-scoped": a gate is repository-controlled code — the one thing
/// on the host that no agent permission surface bounds — and the shell probe is
/// a shell running `exit 0`. Neither runs an agent CLI, so neither is handed
/// the directory an agent's credentials live in, even when the request names an
/// agent. The worker, the review and an agent probe all execute an agent CLI
/// and all need it.
///
/// Exhaustive with no wildcard: a role added later has to be classified here
/// rather than defaulting into the side that hands out credentials.
const fn supplies_credentials(role: &ExecutionRole) -> bool {
    match role {
        ExecutionRole::Implement
        | ExecutionRole::Review
        | ExecutionRole::Probe(ProbeTarget::Agent(_)) => true,
        ExecutionRole::Gate | ExecutionRole::Probe(ProbeTarget::Shell) => false,
    }
}

// ---------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------

/// The `Host` / `host-v1` [`Runner`].
pub struct HostRunner {
    policy: RunnerPolicy,
    digest: String,
    environment: HostEnvironment,
    /// Held for the whole of one `run`, so one `HostRunner` supervises one
    /// process at a time — not a limitation while `Runner::run` is synchronous
    /// and the substrate is sequential.
    ///
    /// Extended notes: `docs/internals/runner/host.md#hostrunnerhooks`
    hooks: Mutex<Box<dyn SpawnHooks + Send>>,
    /// What this runner has already decided a program name is —
    /// `PR6-LANED-001`.
    ///
    /// The lock is held across one get-or-insert of this map and released
    /// before the spawn, so the memo is per-runner state and not a resource
    /// with a lifecycle.
    ///
    /// Per [`HostRunner`] — per *boundary* — so a run's pre-flight and its
    /// attempts execute the same file even when the filesystem moves under
    /// them (DESIGN.md:612), while two runners in one process still each get
    /// their own answer. Process-wide is what
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST` removed.
    ///
    /// Keyed on the **question**, not on the name: the program string together
    /// with the composed `PATH` and `PATHEXT` that answer it. Not on the whole
    /// composed environment, and that is load-bearing rather than an
    /// optimisation — `host-v1` supplies credential locations *role-scoped*
    /// ([`supplies_credentials`]), so a probe's environment and its attempt's
    /// environment differ by design, and a memo keyed on the environment would
    /// miss on exactly the pair DESIGN.md:612 requires to agree.
    ///
    /// Extended notes: `docs/internals/runner/host.md#hostrunnerresolved`
    resolved: Mutex<BTreeMap<ProgramQuestion, Result<PathBuf, String>>>,
}

/// Everything that decides which file a program name is, at one boundary.
///
/// The key of [`HostRunner::resolved`]. `program` is [`CommandSpec::program`]
/// verbatim; `path` and `pathext` are the composed values, `None` when the
/// composed environment does not carry that key at all — which is a different
/// question from carrying it empty, and is why they are `Option` rather than
/// defaulted here.
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
    /// A host runner over this process's environment.
    ///
    /// Infallible because `host-v1`'s record is a constant with nothing to
    /// inspect; [`crate::runner::policy::resolve_host`] is the checked entry
    /// point and returns the same record, which
    /// `new_resolves_the_same_record_as_resolve_host` asserts.
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

    /// A host runner over an explicit environment.
    ///
    /// It does **not** clear [`Self::resolved`], and that is a decision rather
    /// than an omission. The memo is keyed on the question — the program name
    /// with the composed `PATH` and `PATHEXT` — so a new environment that
    /// resolves a name differently asks a different question and misses, and
    /// one that resolves it identically is entitled to the same answer.
    ///
    /// Extended notes: `docs/internals/runner/host.md#hostrunnerwith_environment`
    #[must_use]
    pub fn with_environment(mut self, environment: HostEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Observe (and, for the ST-07 subset, inject at) the containment
    /// sub-effect points of every spawn this runner performs.
    #[must_use]
    pub fn with_hooks(self, hooks: Box<dyn SpawnHooks + Send>) -> Self {
        Self {
            hooks: Mutex::new(hooks),
            ..self
        }
    }

    /// The record this runner declares: `RunnerPolicy{kind: Host, policy:
    /// host-v1, image: None, credential_volumes: None}`.
    ///
    /// Exposed because INV-23 records it in three places — digested into the
    /// marker (P1), in full in the private owner record (P3b), and in
    /// `run_started(4).runner` (P6).
    #[must_use]
    pub const fn policy(&self) -> &RunnerPolicy {
        &self.policy
    }

    /// `runner_policy_sha256` of [`Self::policy`] — the marker's value and the
    /// value every container intent carries.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.digest
    }

    /// The environment contract this runner composes under.
    #[must_use]
    pub const fn environment(&self) -> &HostEnvironment {
        &self.environment
    }

    /// The write command's containment startup step (INV-18, host portion).
    ///
    /// > on Windows every host child is a member of the coordinator's ambient
    /// > kill-on-close Job Object from creation
    /// > … enforced by "ambient job joined at write-command startup (refusal
    /// > otherwise)"
    ///
    /// Called once, **before any spawn** — the contract's
    /// `side_effect_vs_effect_ordering` is "no events; ambient job before any
    /// spawn". On Unix there is nothing to join: containment there is the
    /// per-invocation reaper and the isolated process group, and this returns
    /// `Ok` having done nothing.
    ///
    /// The same step as the free [`contain_write_command`], with **this
    /// runner's** observer attached rather than production's `NoHooks`; it
    /// calls that function rather than repeating it, so there is one join and
    /// one mint in the crate and not two.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] with a diagnostic when the ambient job cannot
    /// be created or joined. The caller refuses the write command before any
    /// effect.
    pub fn start_write_command(&self) -> Result<Contained, UpstrokeError> {
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
        contain_write_command(&mut **hooks)
    }

    /// The `RunnerPreflight` shell probe, executed through this runner.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when the recorded shell cannot be spawned, is
    /// killed by the probe timeout, or does not exit 0.
    pub fn shell_probe(
        &self,
        shell: ShellKind,
        workspace: &Path,
        invocation: InvocationId,
    ) -> Result<(), UpstrokeError> {
        run_shell_probe(self, shell, workspace.to_path_buf(), invocation)
    }

    /// Which file `program` is, at **this** boundary — decided once and then
    /// remembered.
    ///
    /// The `PR6-LANED-001` repair. [`resolve_program`] answers the question by
    /// searching a filesystem; this decides *whether the question is asked*,
    /// and it is asked at most once per [`ProgramQuestion`] per runner. See
    /// [`Self::resolved`] for why per-runner.
    ///
    /// **A refusal is remembered too.** Fail-closed: a run whose pre-flight
    /// could not find `claude` on the `PATH` it composes does not silently find
    /// it at the third attempt because something installed one meanwhile. The
    /// stored value is the refusal's message, and [`UpstrokeError::Refused`]
    /// displays as exactly its message, so the replayed error is the first one
    /// byte for byte.
    ///
    /// Increments [`program_resolutions`] on entry; [`program_searches`] moves
    /// only when the filesystem is reached.
    ///
    /// Extended notes: `docs/internals/runner/host.md#hostrunnerprogram_for`
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] — [`resolve_program`]'s, first-hand or
    /// replayed.
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
        resolved.insert(
            question,
            match &answer {
                Ok(file) => Ok(file.clone()),
                Err(error) => Err(error.to_string()),
            },
        );
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
        // Which file the program *name* is, decided here and nowhere else.
        //
        // **After `compose` and before anything is spawned**, both load-bearing.
        // After, because the environment this resolves against has to be the
        // one the child will run under — a `PATH` the overlay could not have
        // named (it is reserved) but which a caller's `HostEnvironment` decides.
        // Before, because a name that reaches this boundary and resolves to
        // nothing is a pre-flight refusal naming the name, not a `NotFound` from
        // a spawn.
        //
        // **And once per boundary, not once per spawn** (`PR6-LANED-001`,
        // DESIGN.md:612).
        //
        // Extended notes:
        // `docs/internals/runner/host.md#where-the-program-name-is-resolved`
        let program = self.program_for(&request.command.program, &composed)?;
        let mut command = build_command_at(&request.command, &program);
        command.current_dir(&request.workspace);
        // The composed environment *is* the environment: base, reserved
        // values, overlay, and nothing arriving by a route the record does not
        // describe (DESIGN.md:263).
        //
        // Bounded, and named so it cannot grow silently: the base is
        // `std::env::vars_os()`, so anything that iterator does not yield is
        // not inherited either. On Windows that is the `=C:`-style per-drive
        // current-directory variables, which only affect drive-relative paths
        // like `D:sub`. Every process this runner starts is given an absolute
        // `current_dir`, so none of them can be resolving one.
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

/// Build a [`Command`] for a spec whose program the runner has already
/// resolved to a file.
///
/// On Windows the tail after `cmd.exe`'s `/C` or `/K` is handed over raw.
/// `gates::ShellKind::command` explains why, and this is the other half of
/// that rule: "std's Windows quoting escapes embedded quotes as `\"` per
/// `CommandLineToArgvW` rules, which cmd.exe does not un-escape; the /C tail
/// must go through raw_arg to survive intact." A runner that re-quoted the
/// tail would hand every gate command containing a quote to a different
/// program than the one the operator wrote — silently, and only on Windows.
/// `invariants_preserved` says "adapter parsing unchanged", and a gate whose
/// command line changes meaning when it is routed through the Runner is not
/// unchanged.
///
/// Everything else, and every argument on Unix, goes through `Command::arg`.
///
/// The raw-tail rule is keyed on the program **that will execute** rather than
/// on the spec's, so it survives resolution: `cmd`, `cmd.exe` and
/// `C:\Windows\System32\cmd.exe` all have the file stem `cmd`.
///
/// Extended notes: `docs/internals/runner/host.md#build_command_at`
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

/// The index of `cmd.exe`'s `/C` or `/K` switch, when this spec invokes
/// `cmd.exe` at all.
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

// ---------------------------------------------------------------------------
// Program resolution
// ---------------------------------------------------------------------------

mod naming;
use self::naming::{ProgramNaming, composed_value, resolve_program};

thread_local! {
    /// See [`program_resolutions`].
    static RESOLUTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// See [`program_searches`].
    static SEARCHES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many program names **this thread** has resolved at the host boundary.
///
/// Incremented by [`HostRunner::program_for`] on entry, so a spawn that took
/// its answer from the runner's memo and a spawn that searched for it are
/// counted alike: this is "was the program decided for this spawn, and when",
/// not "did the filesystem move". [`program_searches`] is the other question.
///
/// Extended notes:
/// `docs/internals/runner/host.md#program_resolutions-and-program_searches`
#[must_use]
pub fn program_resolutions() -> u64 {
    RESOLUTIONS.with(std::cell::Cell::get)
}

/// How many program names **this thread** has actually searched a filesystem
/// for.
///
/// [`program_resolutions`]'s sibling and the observable of the
/// `PR6-LANED-001` repair: `HostRunner` resolves a name **once per boundary**,
/// not once per spawn (DESIGN.md:612). N spawns of one name through one runner
/// move [`program_resolutions`] by N and this by one.
///
/// Incremented by [`resolve_program`] on entry, so the count moves for a
/// program that names a location as well as for one that is searched for — the
/// question asked is the same, and what differs is the answer.
///
/// Extended notes:
/// `docs/internals/runner/host.md#program_resolutions-and-program_searches`
#[must_use]
pub fn program_searches() -> u64 {
    SEARCHES.with(std::cell::Cell::get)
}

/// The containment proof and its sole mint, in a module with no descendants.
///
/// `Contained`'s field is private to **this** module rather than to
/// `runner::host`, which is the whole point: Rust privacy reaches a module
/// and everything below it, so a field private to `runner::host` is
/// constructible from `runner::host::naming`, `::environment` and `::probe`.
/// `proof` has no children, so its siblings cannot reach the field and the
/// only route to a value is [`contain_write_command`], which performs the
/// join. The mint stays with the type as a local implementation invariant of this
/// module: a proof and the only code that may create it are read together or not
/// at all.
mod proof {
    use super::ESTABLISHMENTS;
    use crate::agent::proc::{self, SpawnHooks};
    use crate::error::UpstrokeError;

    /// Proof that this process has performed its write-command containment
    /// startup (INV-18, host portion).
    ///
    /// The type exists so that "the ambient job is established before anything
    /// this run could spawn" is a thing the compiler checks rather than a thing
    /// each new entry point is trusted to remember. **The field is private to
    /// this module, and this module has no descendants** — so no sibling of
    /// `runner::host`'s children, and not `runner::host` itself, can name it.
    /// The only values of it in the crate are the ones
    /// [`contain_write_command`] returns after [`proc::join_ambient_job`] has
    /// succeeded, and that is enforced by the compiler rather than by a census.
    ///
    /// Extended notes: `docs/internals/runner/host.md#contained`
    #[derive(Debug)]
    pub struct Contained(());

    impl Contained {
        /// The only constructor, and it is private: a token exists exactly when
        /// the containment step ran and returned `Ok`.
        fn new() -> Self {
            ESTABLISHMENTS.with(|count| count.set(count.get() + 1));
            Self(())
        }
    }

    /// The write-command containment startup step (INV-18, host portion), and the
    /// proof that it ran.
    ///
    /// What `src/main.rs` calls at the top of every write command, before any
    /// dispatch arm runs, and what the engine's write coordinator calls before it
    /// touches anything (`crash_reconstruction`).
    ///
    /// A free function because the ambient job is a property of the *process*, not
    /// of a runner value: it is established once at startup and held to process
    /// exit, and it must be established before anything that could spawn exists.
    /// Idempotent for the same reason — `windows_job::join_ambient` memoises the
    /// process's one answer — so a coordinator entered through the CLI, which has
    /// already joined, re-establishes at no cost and gets its proof.
    /// [`crate::runner::host::HostRunner::start_write_command`] is the same step
    /// with a runner's own
    /// observer attached, for the ST-07 evidence, and it calls **this** function —
    /// so there is one join site and one mint in the crate, not two.
    ///
    /// Extended notes:
    /// `docs/internals/runner/host.md#contain_write_command`
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] with a diagnostic when the ambient job cannot be
    /// created or joined. On Unix this cannot fail: containment there is the
    /// per-invocation reaper and the isolated process group.
    pub fn contain_write_command(hooks: &mut dyn SpawnHooks) -> Result<Contained, UpstrokeError> {
        proc::join_ambient_job(hooks).map(|()| Contained::new())
    }
}

pub use self::proof::{Contained, contain_write_command};

thread_local! {
    /// See [`containment_establishments`].
    static ESTABLISHMENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times **this thread** has established write-command containment.
///
/// Incremented by [`Contained::new`], so the count and the tokens cannot
/// disagree.
///
/// Extended notes:
/// `docs/internals/runner/host.md#containment_establishments`
#[must_use]
pub fn containment_establishments() -> u64 {
    ESTABLISHMENTS.with(std::cell::Cell::get)
}

/// [`contain_write_command`] for a caller with nothing to prove it to.
///
/// `src/main.rs` is that caller: it mints its own dispatch-level token from
/// this step's success, and the ordering it needs is between *its* two
/// statements. The engine facade takes the proof instead, because its ordering
/// obligation is against a function it calls.
///
/// It carries the observer through for the same reason [`contain_write_command`]
/// takes one: this is the entry point the CLI's whole write side depends on,
/// and a body that dropped the refusal — `let _ = contain_write_command(hooks);
/// Ok(())` — would leave every `upstroke run` on Windows dispatching with no
/// ambient job and the suite green.
///
/// # Errors
///
/// Whatever [`contain_write_command`] refuses.
pub fn start_write_command(hooks: &mut dyn SpawnHooks) -> Result<(), UpstrokeError> {
    contain_write_command(hooks).map(|_contained| ())
}

/// The shell probe's request, for any [`Runner`].
///
/// A free function because both runners implement the same probe (INV-23);
/// PR6's container runner executes this identical request inside the
/// recorded image.
///
/// The argument vector is taken from [`ShellKind::command`] rather than
/// rebuilt, so the probe runs under exactly the invocation a gate would —
/// including `cmd.exe`'s `/C` and PowerShell's `-NoProfile -NonInteractive`.
/// A probe that spelled the shell differently from the gates would certify
/// something else.
#[must_use]
pub fn shell_probe_request(
    shell: ShellKind,
    workspace: PathBuf,
    invocation: InvocationId,
) -> RunnerRequest {
    RunnerRequest {
        command: shell.spec(SHELL_PROBE_COMMAND),
        workspace,
        // Non-slotted, and the role says so rather than a comment.
        role: ExecutionRole::Probe(ProbeTarget::Shell),
        timeout: SHELL_PROBE_TIMEOUT,
        // No agent: this probe certifies the shell, not a CLI.
        agent: None,
        invocation,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Test-only translation witness. Production construction stays inside
    /// the Process funnel.
    pub(crate) fn build_command(spec: &CommandSpec) -> Command {
        build_command_at(spec, Path::new(&spec.program))
    }
}

#[cfg(test)]
mod tests;
