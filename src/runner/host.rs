//! The host runner: `host-v1`.
//!
//! Everything DESIGN.md:118 gives a runner — "cwd, mounts, environment,
//! supervision, and timeout" — for the boundary that is this machine. It wraps
//! the process funnel in [`crate::agent::proc`] rather than reimplementing it,
//! so "process supervision, timeout, output capture, adapter parsing
//! unchanged" is true by construction and not by parity alone.
//!
//! Three things live here that are not in the funnel:
//!
//! * **Environment composition** (DESIGN.md:258-264). The Upstroke environment is
//!   the base; the runner supplies the reserved keys; `CommandSpec.env` is an
//!   overlay applied last and refused pre-flight if it names a reserved key.
//! * **The `RunnerPreflight` shell probe** (INV-23). The recorded shell
//!   executing `exit 0` **through the Runner**, role `probe(shell)`,
//!   non-slotted, a registered invocation. Availability cannot be established
//!   by inspection; only a spawn establishes it.
//! * **The write-command startup step** (INV-18). On Windows the coordinator
//!   joins its ambient kill-on-close Job Object before any spawn, so every host
//!   child is a member at creation; a failure refuses the write command with a
//!   diagnostic.
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
/// DESIGN.md:260-262: "each supplies role-scoped `HOME`, `PATH`, and
/// credential locations. Adapter overrides may select profiles or CLI
/// behavior but may not conflict with runner-reserved keys."
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
/// `src/capacity.rs:36-37` names two of the three as the vendors' own profile
/// mechanism: "`COPILOT_HOME` (documented) and `CLAUDE_CONFIG_DIR` (works,
/// undocumented as of Aug 2026)"; `CODEX_HOME` is codex-cli's equivalent.
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
    /// process at a time. That is not a limitation today — `Runner::run` is
    /// synchronous until PR11 and the substrate is sequential — but PR11's
    /// concurrent scheduler will need an observer per invocation rather than
    /// per runner, and this is where that shows up.
    hooks: Mutex<Box<dyn SpawnHooks + Send>>,
    /// What this runner has already decided a program name is —
    /// `PR6-LANED-001`.
    ///
    /// DESIGN.md:612: "Probes run through that same runner, **or pre-flight
    /// could certify a host CLI/version different from the one the attempt
    /// executes**." Running the probe through the runner is necessary and is
    /// not sufficient: a bare name re-searched at every spawn can find a
    /// *different file* the second time, because `PATH` is a search order over
    /// a filesystem that moves. `PATH=A:B` with the same name in both, `A/cli`
    /// removed after pre-flight, and the attempt silently runs `B/cli` — one
    /// program string, two executables, and `Caps.version` certifying the one
    /// that did not run.
    ///
    /// So the answer is remembered, and **where** it is remembered is the whole
    /// point. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` removed a process-wide
    /// `OnceLock` in each adapter, which handed one boundary's answer to the
    /// next; putting that back would reintroduce it. This is per
    /// [`HostRunner`] — per *boundary* — so two runners in one process still
    /// each get their own answer, and identity is stable across the one thing
    /// that has to agree: a run's pre-flight and its attempts. Production
    /// constructs exactly one of these per run (`engine::run_harness`) or per
    /// resume (`engine::resume_harness`) and borrows it as `&dyn Runner` for
    /// pre-flight and every attempt, so per-instance *is* per-run;
    /// `production_reaches_a_spawn_through_one_host_runner_per_run` is what
    /// keeps that true.
    ///
    /// Keyed on the **question**, not on the name: the program string together
    /// with the composed `PATH` and `PATHEXT` that answer it. Not on the whole
    /// composed environment, and that is load-bearing rather than an
    /// optimisation — `host-v1` supplies credential locations *role-scoped*
    /// ([`supplies_credentials`]), so a probe's environment and its attempt's
    /// environment differ by design, and a memo keyed on the environment would
    /// miss on exactly the pair DESIGN.md:612 requires to agree. The three
    /// fields that decide the answer are the three the key carries.
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
    /// one that resolves it identically is entitled to the same answer. A clear
    /// here would therefore be a line no fixture could ever see fail, which is
    /// the shape this project treats as debt: the key is the mechanism, and
    /// `a_resolution_question_is_the_program_and_the_environment_that_answers_it`
    /// is what holds it.
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
    /// [`Self::resolved`] for why per-runner rather than per-spawn (a
    /// filesystem that moves between pre-flight and the attempt would otherwise
    /// hand the attempt a different executable under the same name) and why
    /// per-runner rather than process-wide (that is
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST`).
    ///
    /// **A refusal is remembered too.** Fail-closed: a run whose pre-flight
    /// could not find `claude` on the `PATH` it composes does not silently find
    /// it at the third attempt because something installed one meanwhile. The
    /// stored value is the refusal's message, and [`UpstrokeError::Refused`]
    /// displays as exactly its message, so the replayed error is the first
    /// one byte for byte —
    /// `a_refused_name_is_refused_identically_without_asking_the_filesystem_again`
    /// is what holds that rather than this sentence.
    ///
    /// [`program_resolutions`] counts calls here — one per spawn, whether or
    /// not the filesystem was touched. [`program_searches`] counts the ones
    /// that reached [`resolve_program`]. The two are separate because the
    /// ordering predicate ("resolved once per spawn, before any of the spawn")
    /// and the identity predicate ("searched once per boundary") are different
    /// claims and a single counter could not hold both.
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
        // `CommandSpec::program` is a name, not a location, and DESIGN.md:118
        // gives this runner the environment — so the boundary that executes a
        // name is the boundary that says which file it is.
        // `PR4-ADAPTER-RESOLVES-ON-THE-HOST` states the shape in two clauses:
        // "`CommandSpec.program` carries the bare CLI name **and the runner
        // resolves it against the environment it composes**". This is the
        // second clause.
        //
        // **After `compose` and before anything is spawned**, both load-bearing.
        // After, because the environment this resolves against has to be the
        // one the child will run under — a `PATH` the overlay could not have
        // named (it is reserved) but which a caller's `HostEnvironment` decides.
        // Before, because a name that reaches this boundary and resolves to
        // nothing is a pre-flight refusal naming the name, not a `NotFound` from
        // a spawn.
        //
        // **And once per boundary, not once per spawn** (`PR6-LANED-001`).
        // `HostRunner::program_for` searches the first time and remembers the
        // answer for this runner, so a run's pre-flight and its attempts execute
        // the same file even if the filesystem moves under them — DESIGN.md:612,
        // which running the probe through the runner is necessary but not
        // sufficient for.
        let program = self.program_for(&request.command.program, &composed)?;
        let mut command = build_command_at(&request.command, &program);
        command.current_dir(&request.workspace);
        // The composed environment *is* the environment: base, reserved
        // values, overlay, and nothing arriving by a route the record does not
        // describe. "Probe and execution compose the same base, mounts,
        // reserved values, and overlay, so pre-flight certifies the
        // environment that will actually spend" (DESIGN.md:263).
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

/// Turn a [`CommandSpec`] into a [`Command`].
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
/// Build a command for a spec whose program the runner has already resolved
/// to a file.
///
/// The split exists because the resolved program is a [`Path`] and
/// [`CommandSpec::program`] is a `String` (DESIGN.md:222): a `PATH` directory
/// whose name is not valid Unicode is legal on Unix, and writing the resolved
/// path back into the spec would have to either refuse it or rewrite it into a
/// path that names nothing (`PR4-PROGRAM-PATH-NOT-UNICODE`). It never becomes a
/// `String`, so neither happens.
///
/// `cmd.exe`'s raw-tail rule is keyed on the program **that will execute**
/// rather than on the spec's, so it survives resolution: `cmd`, `cmd.exe` and
/// `C:\Windows\System32\cmd.exe` all have the file stem `cmd`, and a gate whose
/// command line changes meaning depending on whether the runner resolved its
/// shell is not "adapter parsing unchanged".
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
/// The same kind of observable as [`containment_establishments`], and here for
/// the ordering rather than the count alone: resolution is specified to happen
/// *once per spawn, before any spawn*, and a suite that proves only that the
/// right file ran holds neither half. A [`SpawnHooks`] observer reading this at
/// the first sub-effect point sees the resolution already done, and sees it done
/// once — see `tests::a_program_is_resolved_once_per_spawn_and_before_any_of_it`.
///
/// Incremented by [`HostRunner::program_for`] on entry, so a spawn that took its
/// answer from the runner's memo and a spawn that searched for it are counted
/// alike: this is "was the program decided for this spawn, and when", not "did
/// the filesystem move". [`program_searches`] is the other question, and the two
/// are separate counters because a single one could not answer both.
#[must_use]
pub fn program_resolutions() -> u64 {
    RESOLUTIONS.with(std::cell::Cell::get)
}

/// How many program names **this thread** has actually searched a filesystem
/// for.
///
/// [`program_resolutions`]'s sibling and the observable of the
/// `PR6-LANED-001` repair: `HostRunner` resolves a name **once per boundary**,
/// not once per spawn, so that a run's pre-flight and its attempts execute the
/// same file even when the filesystem moves between them (DESIGN.md:612). N
/// spawns of one name through one runner move `program_resolutions` by N and
/// this by one.
///
/// It has to be a second counter rather than a reinterpretation of the first,
/// because the two predicates are independently droppable: a memo that never
/// hits satisfies "once per spawn" and reopens :612, and a resolution moved
/// after the first containment point satisfies "once per boundary" and reopens
/// the ordering.
///
/// Incremented by [`resolve_program`] on entry, so the count moves for a
/// program that names a location as well as for one that is searched for — the
/// question asked is the same, and what differs is the answer.
#[must_use]
pub fn program_searches() -> u64 {
    SEARCHES.with(std::cell::Cell::get)
}

/// Proof that this process has performed its write-command containment
/// startup (INV-18, host portion).
///
/// The type exists so that "the ambient job is established before anything
/// this run could spawn" is a thing the compiler checks rather than a thing
/// each new entry point is trusted to remember. Its field is private to this
/// module, so the only values of it in the crate are the ones
/// [`contain_write_command`] returns after [`proc::join_ambient_job`] has
/// succeeded — a caller cannot forge one, and
/// [`crate::engine`]'s write coordinator will not start without one.
///
/// `src/main.rs` has the same shape for the CLI's own dispatch (its
/// `containment::Contained` proves *classification*: a write command joined,
/// a read-only one was not asked to). This is the library half of that idea,
/// for the callers that never go through `main.rs` at all — the frozen public
/// `engine::run/run_with/run_harness` and `resume/resume_with/resume_harness`
/// facades, which a downstream crate may call directly.
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

thread_local! {
    /// See [`containment_establishments`].
    static ESTABLISHMENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times **this thread** has established write-command containment.
///
/// The same kind of observable as [`proc::ambient_job_established`], which
/// exists so INV-18's host portion can be asserted rather than described, and
/// narrower in the two ways that matter for a census of *entry points*:
///
/// * it answers on every platform, where `ambient_job_established` can only
///   answer on Windows — on Unix the step is a no-op, but *whether the step
///   ran* is the same question on both platforms and is the one an entry point
///   is accountable for;
/// * it counts calls on the calling thread instead of latching once per
///   process, so it can say that *this* call established containment. A
///   process-wide latch cannot: the second write coordinator in a process
///   would find it already true whether or not it established anything.
///
/// Incremented by [`Contained::new`], so the count and the tokens cannot
/// disagree.
#[must_use]
pub fn containment_establishments() -> u64 {
    ESTABLISHMENTS.with(std::cell::Cell::get)
}

/// The write-command containment startup step (INV-18, host portion), and the
/// proof that it ran.
///
/// What `src/main.rs` calls at the top of every write command, before any
/// dispatch arm runs, and what the engine's write coordinator calls before it
/// touches anything. `crash_reconstruction`: "at process start every write
/// command creates one non-inheritable ambient Job Object with
/// JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE and assigns the coordinator process
/// itself to it … if the ambient job cannot be created or joined the write
/// command refuses at startup with a diagnostic before any workspace effect".
///
/// A free function because the ambient job is a property of the *process*, not
/// of a runner value: it is established once at startup and held to process
/// exit, and it must be established before anything that could spawn exists.
/// Idempotent for the same reason — `windows_job::join_ambient` memoises the
/// process's one answer — so a coordinator entered through the CLI, which has
/// already joined, re-establishes at no cost and gets its proof.
/// [`HostRunner::start_write_command`] is the same step with a runner's own
/// observer attached, for the ST-07 evidence, and it calls **this** function —
/// so there is one join site and one mint in the crate, not two.
///
/// ## Why the observer is a parameter
///
/// It is threaded one level further out than the funnel's, and for the reason
/// that put a `join` closure inside `proc::join_ambient_job_with` and a
/// `contain` parameter on `engine::run_contained`: **no machine here can make
/// the real join fail on demand** — `windows_job::join_ambient` memoises the
/// process's one answer, so a binary that has ever joined can never observe a
/// failure — and a step that took no observer therefore had no failure path
/// any test could drive.
///
/// That matters here more than anywhere else it is threaded. This is the
/// function the frozen public facades (`engine::run_harness`,
/// `engine::resume_harness`) and `src/main.rs`'s dispatch all reach, and it is
/// the **only** place in the crate that mints a [`Contained`]; the `map` below
/// losing its short-circuit is the whole of INV-18's host portion silently
/// ceasing to hold, on the platform where the failure is real. Production
/// passes [`NoHooks`] at every call site, exactly as it does to the process
/// funnel; the suite arms an observer that refuses at
/// `Spawn.AmbientJobJoined` and watches the refusal come back out with no
/// proof minted (see
/// `tests::the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing`).
///
/// # Errors
///
/// [`UpstrokeError::Refused`] with a diagnostic when the ambient job cannot be
/// created or joined. On Unix this cannot fail: containment there is the
/// per-invocation reaper and the isolated process group.
pub fn contain_write_command(hooks: &mut dyn SpawnHooks) -> Result<Contained, UpstrokeError> {
    proc::join_ambient_job(hooks).map(|()| Contained::new())
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
/// A free function because both runners implement the same probe — INV-23:
/// "the RunnerPreflight — one non-slotted shell probe (the recorded shell
/// executing `exit 0`) and one slotted probe per recorded agent, each a
/// registered invocation through the run's Runner". PR6's container runner
/// executes this identical request inside the recorded image.
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
