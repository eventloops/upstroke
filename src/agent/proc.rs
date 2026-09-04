//! Subprocess supervision: run a command, feed stdin, drain both pipes
//! concurrently (required on Windows — a full pipe buffer deadlocks a child
//! that is still writing), and enforce a wall-clock timeout. The synchronous
//! runner remains until the Tokio scheduler arrives in v0.2.
//!
//! Windows subtleties this module owns: `.cmd` shims (npm installs) mean the
//! direct child is `cmd.exe`, so every invocation is placed in a private Job
//! Object before its suspended primary thread is allowed to execute. Closing
//! that handle kills ordinary descendants even when the direct child exits
//! successfully or Upstroke is terminated. Explicit cleanup uses the same job
//! and a bounded wait; it never shells out to a PID-based tree walker. Any
//! process that inherited a pipe handle must not be able to stall the drain —
//! readers accumulate into shared buffers that are snapshotted after a bounded
//! grace instead of joined unconditionally.
//!
//! Unix subtleties are the mirror image: each invocation gets an isolated
//! process group so a timeout can kill every member, but that isolation
//! also stops terminal interrupts reaching the child automatically. A tiny
//! process-wide signal monitor below preserves inherited ignored and custom
//! handlers,
//! coordinates SIGINT/SIGTERM/SIGHUP/SIGQUIT termination, and proxies terminal
//! suspension/continuation. It waits out any spawn-registration race, blocks
//! launches across a suspension transition, and uses a descriptor-scrubbed
//! guard process to close the last signal-to-stop race. A separate cleanup
//! reaper survives even an uncatchable Upstroke SIGKILL. Together the monitor and
//! reaper stop and clean every active process group before ownership is
//! released. A host runner does not claim to contain code that deliberately
//! leaves that group with `setsid`/`setpgid`; the external/container runner
//! described in DESIGN.md is the boundary for hostile or daemonising repository
//! code. Pretending otherwise would require racy process-table inference on
//! macOS, where there is no unprivileged descendant-containment primitive.
//! Within the host-runner contract, run ownership cannot be handed to a resume
//! -- or appear suspended -- while an isolated agent group is running.
// PROCESS FUNNEL: this module is in the **funnel section** of
// `effects/allowlist.toml`. Its effectful entries take ProcessSite by value;
// `runner::host` constructs commands and passes both Spawn and Terminate sites
// into this supervision boundary. `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::topology::effects::ProcessSite;

use crate::error::UpstrokeError;
use crate::topology::effects::SubEffectPoint;

/// The observation and injection surface, out of line in `proc/hooks.rs`.
///
/// Private module, explicit re-exports: `crate::agent::proc::SpawnHooks` and
/// `crate::agent::proc::NoHooks` keep the exact paths and the exact
/// visibilities they had inline, and the two appliers keep theirs -- visible in
/// `proc` and its descendants, which is what a private item of `proc` was.
/// [`memoised_outcome`] stays here: it is the funnel's own no-degraded-mode
/// contract rather than part of the injection surface, `windows_job` reaches it
/// by that name, and the arm it decides is exercised on every platform.
mod hooks;
pub use self::hooks::{NoHooks, SpawnHooks};
// Each applier is reached only from the arm that has containment points to
// apply an answer at: the four Unix ones inside the bounded supervision entry
// point below, and `windows_job`'s three. An ungated import here is an unused
// one on the other platform, which `-D warnings` makes an error.
#[cfg(unix)]
use self::hooks::apply;
#[cfg(windows)]
use self::hooks::apply_io;

/// What a **memoised** one-shot establishment reports to a caller.
///
/// A `OnceLock` holding a `Result` has exactly two arms and one of them is not
/// otherwise reachable in a test: the coordinator joins one ambient job for its
/// whole life, so a process that memoised a success can never observe a failure
/// and a process that memoised a failure never got a coordinator. Every ambient
/// failure this suite can build is the *injected* one, which fires strictly
/// before the memo is consulted — so `Err(_) => Ok(())` here left the whole
/// suite green while a Windows coordinator whose `CreateJobObjectW` failed
/// carried on into `run`/`resume` with no ambient kill-on-close job at all: the
/// degraded mode `crash_reconstruction` forbids ("no degraded mode; deferred")
/// and `expected_failures_refusals[1]` requires a startup refusal for
/// (`PR5-CORRECTNESS-010`).
///
/// Generic and platform-independent **so that arm can be executed on any
/// machine**. The value it decides about is Windows-only; the decision is not,
/// and a decision only one platform can test is a decision one platform never
/// tests.
///
/// # Errors
///
/// The memoised diagnostic, verbatim — the caller renders it into the refusal,
/// so a *fresh* message here would name something that did not happen.
// Unix has no ambient job and therefore no production caller; the test in
// `proc/tests.rs` is the only one there, and running it there is the point.
// `dead_code` is not a governed lint (`effects::GOVERNED_LINTS`), so this is
// outside the allow-placement scan rather than an exception to it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn memoised_outcome<T>(memo: &Result<T, String>) -> Result<(), String> {
    match memo {
        Ok(_) => Ok(()),
        Err(message) => Err(message.clone()),
    }
}

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// Exit code if the process exited normally; `None` when killed for a
    /// timeout/output limit or terminated by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Wall clock from spawn to process exit (not including pipe drain).
    pub duration: Duration,
    pub timed_out: bool,
    /// The child exceeded the bounded stdout or stderr capture allowance and
    /// its owned process tree was terminated.
    pub output_limited: bool,
}

/// How long to keep draining pipes after the process is gone. Normally EOF is
/// immediate; the grace only caps the pathological case of an orphaned
/// grandchild still holding a write handle.
const DRAIN_GRACE_EXIT: Duration = Duration::from_secs(2);
const DRAIN_GRACE_KILL: Duration = Duration::from_millis(500);
/// Per stream. Readers continue draining after this point so the child cannot
/// block on a full pipe while the supervisor notices and terminates its tree.
const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// A direct child plus the platform primitive that owns its ordinary
/// descendants. Keeping ownership beside `Child` prevents a successful wait
/// from accidentally bypassing tree settlement.
struct ProcessTree {
    child: Child,
    #[cfg(windows)]
    job: windows_job::Job,
}

impl ProcessTree {
    fn spawn(command: &mut Command, hooks: &mut dyn SpawnHooks) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            let (child, job) = windows_job::spawn_suspended_in_job(command, hooks)?;
            Ok(Self { child, job })
        }
        #[cfg(not(windows))]
        {
            let child = command.spawn()?;
            hooks.child_created(child.id());
            Ok(Self { child })
        }
    }

    /// The direct child has already exited. Windows descendants remain job
    /// members, so terminate and observe the job empty before returning its
    /// status. Unix process-group settlement is owned by `termination`.
    #[cfg(windows)]
    fn finish_direct_exit(&mut self) -> Result<(), UpstrokeError> {
        self.job
            .terminate_and_wait()
            .map_err(|error| UpstrokeError::Agent {
                message: format!("settling the Windows agent job after direct-child exit: {error}"),
            })?;
        Ok(())
    }
}

impl Deref for ProcessTree {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl DerefMut for ProcessTree {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

/// Run `command` through the site-authoritative process funnel, with its
/// containment sub-effect points observable.
///
/// `spawn_site` and `terminate_site` are validated before any process effect;
/// timeout and output-limit cleanup carry the validated termination site into
/// the platform primitive. `stdin_data` is bytes because a
/// [`crate::runner::CommandSpec`] carries bytes.
///
/// # Errors
///
/// Spawn failure, supervision failure, or a fault the observer injected.
pub fn run_with_timeout_at(
    spawn_site: ProcessSite,
    terminate_site: ProcessSite,
    command: Command,
    stdin_data: &[u8],
    timeout: Duration,
    hooks: &mut dyn SpawnHooks,
) -> Result<ProcessOutput, UpstrokeError> {
    validate_process_sites(spawn_site, terminate_site)?;
    run_with_timeout_and_limit(
        spawn_site,
        terminate_site,
        command,
        stdin_data,
        timeout,
        OUTPUT_LIMIT_BYTES,
        hooks,
    )
}

fn validate_process_sites(
    spawn_site: ProcessSite,
    terminate_site: ProcessSite,
) -> Result<(), UpstrokeError> {
    match (spawn_site, terminate_site) {
        (ProcessSite::Spawn, ProcessSite::Terminate) => Ok(()),
        _ => Err(UpstrokeError::Agent {
            message: format!(
                "process funnel requires (Process.Spawn, Process.Terminate), got ({}, {})",
                spawn_site.name(),
                terminate_site.name()
            ),
        }),
    }
}

fn run_with_timeout_and_limit(
    spawn_site: ProcessSite,
    terminate_site: ProcessSite,
    mut command: Command,
    stdin_data: &[u8],
    timeout: Duration,
    output_limit: usize,
    hooks: &mut dyn SpawnHooks,
) -> Result<ProcessOutput, UpstrokeError> {
    validate_process_sites(spawn_site, terminate_site)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Enter before `spawn`: if an interrupt arrives in the narrow interval
    // between creating the child and learning its pid, the signal monitor
    // waits for this registration rather than terminating Upstroke first and
    // orphaning the new process group.
    #[cfg(unix)]
    let mut termination = termination::Supervisor::begin(terminate_site)?;
    // `Spawn.ReaperStarted`: "fork of the per-invocation reaper, which takes
    // its shared cleanup hold R28". `begin` returning Ok is exactly that
    // having happened, and nothing else in this function can have happened
    // yet.
    #[cfg(unix)]
    apply(
        hooks.point(SubEffectPoint::ReaperStarted),
        SubEffectPoint::ReaperStarted,
    )?;
    #[cfg(unix)]
    termination.prepare(&mut command);

    let started = Instant::now();
    let mut child = ProcessTree::spawn(&mut command, hooks).map_err(|e| UpstrokeError::Agent {
        message: format!(
            "failed to spawn `{}`: {e}",
            command.get_program().to_string_lossy()
        ),
    })?;
    // `Spawn.PreExecPgidAndRegister`. Two coordinates, and they are not the
    // same one:
    //
    // * The **operation** is in the forked child before `exec` — `setpgid(0,0)`
    //   and the reaper registration, in `termination::Supervisor::prepare`'s
    //   `pre_exec` closure. That is where the packet puts it ("in the child
    //   before exec") and where it is.
    // * The **injection** is here, parent-side, immediately after `spawn`
    //   returns `Ok`. This point's only declared mode is `Kill`
    //   (`SubEffectPoint::modes`), a kill is a *coordinator* death, and the
    //   packet's claim for it — "a coordinator kill after any of these leaves a
    //   group the reaper settles while holding R28" — is true only once the
    //   child exists and its group does. A kill delivered inside the forked
    //   child would end the fork, not the coordinator, and would leave no group
    //   at all. An observer hook cannot run there in any case: after `fork` in a
    //   multithreaded process only async-signal-safe calls are permitted, and
    //   every real observer locks and allocates. The packet contemplates
    //   exactly this: "these are parent-side **or** pre-exec points the harness
    //   controls".
    //
    // Fired unconditionally, because `spawn` returning `Ok` *is* the evidence
    // the closure ran: `std` reports a `pre_exec` error through the child's
    // CLOEXEC status pipe and returns `Err`. The kernel oracle
    // (`child_leads_its_own_group`) is a second, independent witness and lives
    // in the tests — as a guard here it could only ever produce a false
    // negative, silently dropping the point for a child that left its own group
    // after `exec` (DESIGN.md:398-402 puts such a process outside host
    // guarantees; it does not make it invisible).
    #[cfg(unix)]
    apply(
        hooks.point(SubEffectPoint::PreExecPgidAndRegister),
        SubEffectPoint::PreExecPgidAndRegister,
    )?;
    // `Spawn.Exec`: `Command::spawn` reports a failed `execvp` through its own
    // CLOEXEC status pipe and returns `Err`, so reaching here is the exec
    // having succeeded.
    #[cfg(unix)]
    apply(hooks.point(SubEffectPoint::Exec), SubEffectPoint::Exec)?;
    #[cfg(unix)]
    if let Err(error) = termination.register(child.id()) {
        // Drop the pre-exec reaper first: it still has an anchor pinning this
        // child's group identity and will kill every member before returning.
        drop(termination);
        kill_tree(terminate_site, &mut child)?;
        return Err(error);
    }
    // `Spawn.Registered`: "parent-side registration".
    #[cfg(unix)]
    apply(
        hooks.point(SubEffectPoint::Registered),
        SubEffectPoint::Registered,
    )?;

    // Feed stdin from its own thread: the child may not read stdin until it
    // has written output, and this thread must not block the pipe drains.
    let stdin_bytes = stdin_data.to_vec();
    let stdin_handle = child.stdin.take();
    let stdin_thread = thread::spawn(move || {
        if let Some(mut pipe) = stdin_handle {
            // A child that exits without reading stdin breaks the pipe; that
            // is its prerogative, not an error.
            let _ = pipe.write_all(&stdin_bytes);
        }
    });

    let stdout_drain = child
        .stdout
        .take()
        .map(|pipe| Drain::start(pipe, output_limit));
    let stderr_drain = child
        .stderr
        .take()
        .map(|pipe| Drain::start(pipe, output_limit));

    let mut timed_out = false;
    let mut output_limited = false;
    #[cfg(unix)]
    let code = loop {
        match child_exited_unreaped(&child) {
            Ok(true) => {
                // Leave the exited leader as a zombie until cleanup completes:
                // its PID pins the PGID, so no unrelated group can reuse the
                // numeric id between observation and the final signal.
                if let Err(error) = termination.finish() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
                let status = child.wait().map_err(|e| UpstrokeError::Agent {
                    message: format!("reaping agent process: {e}"),
                })?;
                break status.code();
            }
            Ok(false) => {
                if drain_limit_exceeded(&stdout_drain, &stderr_drain) {
                    output_limited = true;
                    if let Err(error) = termination.finish() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                } else if started.elapsed() >= timeout {
                    timed_out = true;
                    if let Err(error) = termination.finish() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let cleanup = termination.finish();
                let _ = child.kill();
                let _ = child.wait();
                cleanup?;
                return Err(UpstrokeError::Agent {
                    message: format!("waiting on agent process: {e}"),
                });
            }
        }
    };
    #[cfg(not(unix))]
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                child.finish_direct_exit()?;
                break status.code();
            }
            Ok(None) => {
                if drain_limit_exceeded(&stdout_drain, &stderr_drain) {
                    output_limited = true;
                    kill_tree(terminate_site, &mut child)?;
                    break None;
                } else if started.elapsed() >= timeout {
                    timed_out = true;
                    kill_tree(terminate_site, &mut child)?;
                    break None;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                kill_tree(terminate_site, &mut child)?;
                return Err(UpstrokeError::Agent {
                    message: format!("waiting on agent process: {e}"),
                });
            }
        }
    };
    let duration = started.elapsed();

    let grace = if timed_out || output_limited {
        DRAIN_GRACE_KILL
    } else {
        DRAIN_GRACE_EXIT
    };
    // Bounded like the read drains: a prompt larger than the pipe buffer plus
    // an orphan holding the read end would otherwise block write_all forever
    // and hang the supervisor past its own timeout. Abandoning the thread is
    // safe — it owns its handle and exits when the last reader closes.
    let stdin_deadline = Instant::now() + grace;
    while !stdin_thread.is_finished() && Instant::now() < stdin_deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if stdin_thread.is_finished() {
        let _ = stdin_thread.join();
    }
    let (stdout, stdout_limited) = stdout_drain.map(|d| d.collect(grace)).unwrap_or_default();
    let (stderr, stderr_limited) = stderr_drain.map(|d| d.collect(grace)).unwrap_or_default();
    output_limited |= stdout_limited || stderr_limited;

    Ok(ProcessOutput {
        code,
        stdout,
        stderr,
        duration,
        timed_out,
        output_limited,
    })
}

/// The pipe reader, out of line in `proc/drain.rs`, with the predicate the
/// supervisor asks it for. Both are visible in `proc` and its descendants,
/// exactly as they were when they were private items of this file.
mod drain;
use self::drain::{Drain, drain_limit_exceeded};

/// Kill the whole process tree. Killing only the direct child is not enough
/// when it is a `cmd.exe` shim: the real agent process would survive, keep
/// running, and keep the pipes open.
fn kill_tree(terminate_site: ProcessSite, child: &mut ProcessTree) -> Result<(), UpstrokeError> {
    debug_assert_eq!(terminate_site, ProcessSite::Terminate);
    #[cfg(windows)]
    {
        let cleanup = child.job.terminate_and_wait();
        let _ = child.kill();
        let _ = child.wait();
        cleanup.map_err(|error| UpstrokeError::Agent {
            message: format!("terminating the Windows agent job: {error}"),
        })
    }
    #[cfg(not(windows))]
    {
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: `run_with_timeout` put this child in a new process group
            // whose id is the child's pid. A negative pid targets that group only.
            let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    }
}

/// Whether `pid` leads its own Unix process group.
///
/// The independent witness that `Spawn.PreExecPgidAndRegister`'s operation ran
/// in the forked child. Asks the kernel, not this crate: `getpgid(pid) == pid`
/// is true exactly when the pre-exec closure's `setpgid(0, 0)` ran. A child
/// that has exited but not been reaped still answers, because its pid is
/// pinned by the zombie.
///
/// Test-only on purpose: as a production guard it could only ever *withhold*
/// the point, never add information (see the comment at the injection
/// coordinate).
#[cfg(all(unix, test))]
pub(crate) fn child_leads_its_own_group(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: `getpgid` reads process-table state for a pid this process owns
    // as a child and has not reaped; it borrows nothing.
    let pgid = unsafe { libc::getpgid(pid) };
    pgid == pid
}

/// The ambient Job Object and the reclaim scope, out of line in
/// `proc/ambient.rs`. Private module, explicit re-exports: every path under
/// `crate::agent::proc::` that named one of these before the split names the
/// same item now, at the same visibility.
mod ambient;
#[cfg(all(windows, test))]
pub(crate) use self::ambient::poison_ambient_for_tests;
pub use self::ambient::{
    AMBIENT_REFUSAL_PREFIX, AMBIENT_REFUSAL_SIMULATED, join_ambient_job,
    set_container_reclaim_scope,
};
#[cfg(windows)]
pub use self::ambient::{
    ambient_job_established, child_in_ambient_job, process_alive, process_creation_time,
};

#[cfg(windows)]
mod windows_job {
    use std::io;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::ptr;
    use std::thread;
    use std::time::{Duration, Instant};

    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, GetCurrentProcess, GetProcessTimes, OpenProcess, OpenThread,
        PROCESS_QUERY_LIMITED_INFORMATION, ResumeThread, THREAD_SUSPEND_RESUME,
        WaitForSingleObject,
    };

    use super::{SpawnHooks, SubEffectPoint, apply_io};

    const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

    /// A non-inheritable Job Object configured before any supervised code can
    /// run. The OS closes this handle on abrupt conductor death, and
    /// KILL_ON_JOB_CLOSE then terminates every ordinary descendant.
    pub(super) struct Job {
        handle: HANDLE,
    }

    /// The real `CreateJobObjectW`, as [`Job::create`] passes it.
    fn real_create_job() -> HANDLE {
        // SAFETY: null security attributes and name request an unnamed,
        // non-inheritable job owned solely by this process.
        unsafe {
            windows_sys::Win32::System::JobObjects::CreateJobObjectW(ptr::null(), ptr::null())
        }
    }

    /// The real `SetInformationJobObject`, as [`Job::create`] passes it.
    fn real_configure_job(
        handle: HANDLE,
        limits: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        size: u32,
    ) -> i32 {
        // SAFETY: `limits` has exactly the layout and lifetime required by
        // JobObjectExtendedLimitInformation; `handle` is live.
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                ptr::from_ref(limits).cast(),
                size,
            )
        }
    }

    /// The real `TerminateJobObject`, as [`Job::terminate_and_wait`] passes it.
    fn real_terminate_job(handle: HANDLE) -> i32 {
        // SAFETY: the handle remains live for this call and the requested exit
        // code has no semantic meaning outside this private job.
        unsafe { TerminateJobObject(handle, 1) }
    }

    /// The real `QueryInformationJobObject`, as the accounting callers pass it.
    fn real_query_accounting(
        handle: HANDLE,
        accounting: &mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    ) -> i32 {
        // SAFETY: the output buffer is correctly typed and sized and the
        // optional returned-length pointer is not needed.
        #[expect(clippy::expect_used, reason = "a fixed Win32 struct size fits in u32")]
        unsafe {
            QueryInformationJobObject(
                handle,
                JobObjectBasicAccountingInformation,
                ptr::from_mut(accounting).cast(),
                u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                    .expect("job accounting structure fits in u32"),
                ptr::null_mut(),
            )
        }
    }

    impl Job {
        fn create() -> io::Result<Self> {
            Self::create_with(real_create_job, real_configure_job)
        }

        /// [`Job::create`] over the two Win32 calls it makes.
        ///
        /// The same reason `create_ambient` takes its assignment call: on a
        /// working machine `CreateJobObjectW` and `SetInformationJobObject`
        /// always succeed, so both failure branches are unreachable in every
        /// real test and either could be inverted with the whole suite green —
        /// while `crash_reconstruction`'s "if the ambient job cannot be
        /// **created** or joined the write command refuses at startup" and
        /// INV-18's "refusal before any effect if the ambient job cannot be
        /// established" silently stopped holding. The join had a seam; these
        /// two did not, which made the guarantee asserted for one third of the
        /// sentence that states it.
        ///
        /// `configure` is handed the limit structure rather than a raw
        /// pointer, so a test can also read what is being asked for:
        /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the whole fail-safe, and a
        /// job configured with any other flag would still return success here.
        fn create_with(
            create: impl FnOnce() -> HANDLE,
            configure: impl FnOnce(HANDLE, &JOBOBJECT_EXTENDED_LIMIT_INFORMATION, u32) -> i32,
        ) -> io::Result<Self> {
            let handle = create();
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self { handle };
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            #[expect(clippy::expect_used, reason = "a fixed Win32 struct size fits in u32")]
            let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .expect("job information structure fits in u32");
            if configure(job.handle, &limits, size) == 0 {
                // `job` drops here, closing the handle: an unconfigured job is
                // not a job this process may keep.
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        pub(super) fn terminate_and_wait(&self) -> io::Result<()> {
            self.terminate_and_wait_with(real_terminate_job, real_query_accounting)
        }

        /// [`Job::terminate_and_wait`] over the Win32 calls it makes.
        ///
        /// DESIGN.md:402 — "Direct-child success and timeout both terminate
        /// and **boundedly observe that job empty**". Both halves of that
        /// sentence are unobservable from outside on a working machine: a real
        /// job empties immediately, so an implementation that skipped the
        /// observation entirely, and one that observed without a bound, both
        /// return promptly and leave nothing behind for a test to see. The
        /// accounting seam is what makes "observe" and "bounded" separate
        /// facts.
        pub(super) fn terminate_and_wait_with(
            &self,
            terminate: impl FnOnce(HANDLE) -> i32,
            query: impl Fn(HANDLE, &mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION) -> i32,
        ) -> io::Result<()> {
            if terminate(self.handle) == 0 {
                return Err(io::Error::last_os_error());
            }
            let deadline = Instant::now() + CLEANUP_TIMEOUT;
            loop {
                if self.active_processes_with(&query)? == 0 {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Windows agent job did not become empty within 2 seconds",
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        /// How many processes the job still holds, over the Win32 call that
        /// answers.
        ///
        /// R22's release is "released on exit, timeout kill, cancel, or
        /// shutdown (private Job Object / process group)", and this is the only
        /// thing that reports whether the release happened. A query error read
        /// as "empty" would report a job settled while it still held a live
        /// member, so the error branch is the accounting, not an aside — and it
        /// is unreachable without a seam.
        pub(super) fn active_processes_with(
            &self,
            query: impl Fn(HANDLE, &mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION) -> i32,
        ) -> io::Result<u32> {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            if query(self.handle, &mut accounting) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(accounting.ActiveProcesses)
        }

        /// Whether `pid` is a member of **this** job, asked of the kernel.
        ///
        /// The Windows counterpart of `child_leads_its_own_group`, and
        /// test-only for the same reason: an independent oracle for the
        /// private job's identity, not a production guard. `IsProcessInJob`
        /// answers from the process table, so it cannot agree with a spawn path
        /// that never assigned anything.
        #[cfg(test)]
        pub(super) fn contains(&self, pid: u32) -> Option<bool> {
            job_contains(self.handle, pid)
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: this object uniquely owns the non-inheritable handle.
            // KILL_ON_JOB_CLOSE is the final fail-safe if explicit settlement
            // returned an error or the conductor is being torn down.
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }

    pub(super) fn spawn_suspended_in_job(
        command: &mut Command,
        hooks: &mut dyn SpawnHooks,
    ) -> io::Result<(Child, Job)> {
        spawn_suspended_in_job_with(command, hooks, real_assign_to_job, resume_only_thread)
    }

    /// The real `AssignProcessToJobObject`, as [`spawn_suspended_in_job`]
    /// passes it.
    pub(super) fn real_assign_to_job(job: HANDLE, process: HANDLE) -> i32 {
        // SAFETY: `Child` owns a live process handle and `job` is live; both
        // are process-wide kernel object references, not borrowed memory.
        unsafe { AssignProcessToJobObject(job, process) }
    }

    /// Whether `pid` is a member of the job `job`, asked of the kernel.
    ///
    /// See [`Job::contains`]; this is the same query for a handle a test
    /// captured through the assignment seam rather than for a live [`Job`],
    /// because constructing a second `Job` over the same handle would close it
    /// on drop.
    #[cfg(test)]
    pub(super) fn job_contains(job: HANDLE, pid: u32) -> Option<bool> {
        let process = OpenHandle::open(pid)?;
        let mut member = 0;
        // SAFETY: both handles are live and `member` is a writable BOOL.
        let queried = unsafe { IsProcessInJob(process.0, job, &raw mut member) };
        if queried == 0 {
            return None;
        }
        Some(member != 0)
    }

    /// [`spawn_suspended_in_job`] over the two Win32 steps that come after
    /// creation.
    ///
    /// Both always succeed on a working machine, so the two cleanup branches
    /// that follow them — terminate the private job, kill the child, wait for
    /// it — are unreachable in every real test, and R22's "created as an
    /// ambient-job member, so a coordinator death at any spawn sub-step incl.
    /// the create-suspended prefix terminates it" was asserted for the ambient
    /// job and not for the spawn path's own recovery.
    ///
    /// `assign` is also what makes the `PrivateJobAssigned` coordinate
    /// checkable: it hands a test the private job's handle at the instant the
    /// assignment is made, so the hook can be measured against the operation it
    /// is named for rather than against the other hooks.
    pub(super) fn spawn_suspended_in_job_with(
        command: &mut Command,
        hooks: &mut dyn SpawnHooks,
        assign: impl FnOnce(HANDLE, HANDLE) -> i32,
        resume: impl FnOnce(u32) -> io::Result<()>,
    ) -> io::Result<(Child, Job)> {
        let job = Job::create()?;
        command.creation_flags(CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        hooks.child_created(child.id());
        // `Spawn.CreatedSuspended`: "the child is already an ambient-job
        // member". This is the window the ambient job exists to close -- a
        // coordinator killed here leaves a suspended process that no private
        // job owns -- so it is where the kill injection goes.
        if let Err(error) = apply_io(
            hooks.point(SubEffectPoint::CreatedSuspended),
            SubEffectPoint::CreatedSuspended,
        ) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        // The primary thread is still suspended, so candidate code cannot
        // create an escaping child between process creation and assignment to
        // the job.
        let assigned = assign(job.handle, child.as_raw_handle() as HANDLE);
        if assigned == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if let Err(error) = apply_io(
            hooks.point(SubEffectPoint::PrivateJobAssigned),
            SubEffectPoint::PrivateJobAssigned,
        ) {
            let _ = job.terminate_and_wait();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if let Err(error) = resume(child.id()) {
            let _ = job.terminate_and_wait();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if let Err(error) = apply_io(
            hooks.point(SubEffectPoint::Resumed),
            SubEffectPoint::Resumed,
        ) {
            let _ = job.terminate_and_wait();
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, job))
    }

    /// The coordinator's ambient kill-on-close Job Object.
    ///
    /// A process-wide singleton, and never dropped: `OnceLock` in a `static`
    /// has no destructor, so the handle survives to process exit. That is the
    /// requirement, not an accident -- the coordinator is itself a member, so
    /// closing this handle terminates the coordinator.
    static AMBIENT: OnceLock<Result<AmbientJob, String>> = OnceLock::new();

    /// The ambient job's handle, held for the life of the process.
    ///
    /// A separate type from [`Job`] and not merely a second value of it,
    /// because the two have opposite ownership rules. `Job` is owned by the
    /// thread supervising one invocation and its `Drop` is load-bearing --
    /// closing it is how a timeout settles the tree. This one is shared by
    /// every thread and must never be closed, so it has no `Drop` at all.
    #[derive(Debug)]
    struct AmbientJob(HANDLE);

    // SAFETY: a Windows `HANDLE` is a process-wide reference to a kernel
    // object, not a pointer into this process's memory. The only calls made on
    // this one -- `AssignProcessToJobObject` and `IsProcessInJob` -- are
    // thread-safe, the value is never mutated after the `OnceLock` is set, and
    // it is never closed.
    unsafe impl Send for AmbientJob {}
    // SAFETY: as above.
    unsafe impl Sync for AmbientJob {}

    /// Create the ambient job and put this process in it, once.
    ///
    /// The memo is decided by [`super::memoised_outcome`] rather than by a
    /// `match` here, because that arm is unreachable in this process once
    /// either answer has been taken. See its documentation.
    pub(super) fn join_ambient() -> Result<(), String> {
        super::memoised_outcome(AMBIENT.get_or_init(|| {
            // SAFETY: `GetCurrentProcess` is the documented pseudo-handle for
            // this process and the job handle is live. Windows 8 and later
            // nest jobs, so an existing job (cargo's, a CI runner's, an
            // OpenSSH session's) is a parent of this one rather than a
            // conflict.
            create_ambient(|job, process| unsafe { AssignProcessToJobObject(job, process) })
        }))
    }

    /// Memoise an ambient **failure** before anything has joined, so a test
    /// process can carry a real one through [`join_ambient`].
    ///
    /// Spends the process's one ambient cell, so it belongs only in a
    /// subprocess helper. Returns whether the cell was still free.
    #[cfg(test)]
    pub(super) fn poison_ambient_for_tests(message: &str) -> bool {
        AMBIENT.set(Err(message.to_owned())).is_ok()
    }

    /// The body of [`join_ambient`], over the assignment call it makes.
    ///
    /// `assign` is a parameter for one reason: `AssignProcessToJobObject`
    /// returns a Win32 `BOOL`, where **zero is failure and every other value
    /// — including `-1` — is success**, and on a working machine it always
    /// returns success. So the branch that reads it is unreachable in every
    /// real test, and `if joined == 0` could be `if joined == -1` with the
    /// whole suite green while `crash_reconstruction`'s "if the ambient job
    /// cannot be created or joined the write command refuses at startup"
    /// silently stopped holding.
    ///
    /// Not memoised, and it does not touch [`AMBIENT`]: a test may call this
    /// with a refusing `assign` without spending the process's one ambient
    /// job.
    fn create_ambient(assign: impl Fn(HANDLE, HANDLE) -> i32) -> Result<AmbientJob, String> {
        create_ambient_with(Job::create, assign)
    }

    /// [`create_ambient`] over the job it creates as well as the assignment.
    ///
    /// `crash_reconstruction` names two failures and this slice's contract
    /// names them together — "ambient job cannot be **created** or joined
    /// (Windows) → write command refuses at startup with a diagnostic". The
    /// join half had a seam and the creation half did not, so the branch that
    /// turns a failed `CreateJobObjectW` or `SetInformationJobObject` into a
    /// refusal was unreachable: `create_ambient` could have returned a disabled
    /// job and continued, and the whole suite would have stayed green while the
    /// coordinator ran with no ambient job at all.
    fn create_ambient_with(
        make_job: impl FnOnce() -> io::Result<Job>,
        assign: impl Fn(HANDLE, HANDLE) -> i32,
    ) -> Result<AmbientJob, String> {
        let job = make_job().map_err(|error| format!("it could not be created ({error})"))?;
        // SAFETY: `GetCurrentProcess` is the documented pseudo-handle for this
        // process and the job handle is live.
        let joined = assign(job.handle, unsafe { GetCurrentProcess() });
        if joined == 0 {
            // `job` drops here, closing the handle: a kill-on-close job
            // with no members terminates nothing.
            return Err(format!(
                "this process could not join it ({})",
                io::Error::last_os_error()
            ));
        }
        // Joined. From here the handle must outlive every `Drop` in this
        // process, because closing it terminates this process.
        let job = std::mem::ManuallyDrop::new(job);
        Ok(AmbientJob(job.handle))
    }

    /// Whether the ambient job has been established in this process.
    pub(super) fn ambient_established() -> bool {
        matches!(AMBIENT.get(), Some(Ok(_)))
    }

    /// Whether `pid` is a member of this process's ambient job.
    ///
    /// `None` when no ambient job has been established, or the process cannot
    /// be opened. The kernel answers, so this is an oracle independent of the
    /// spawn path it checks.
    pub(super) fn ambient_contains(pid: u32) -> Option<bool> {
        let Some(Ok(job)) = AMBIENT.get() else {
            return None;
        };
        let process = OpenHandle::open(pid)?;
        let mut member = 0;
        // SAFETY: both handles are live and `member` is a writable BOOL.
        let queried = unsafe { IsProcessInJob(process.0, job.0, &raw mut member) };
        if queried == 0 {
            return None;
        }
        Some(member != 0)
    }

    /// A borrowed process handle with query and synchronise rights.
    struct OpenHandle(HANDLE);

    impl OpenHandle {
        fn open(pid: u32) -> Option<Self> {
            // SAFETY: no borrowed inputs; a failure returns null.
            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
            if handle.is_null() {
                return None;
            }
            Some(Self(handle))
        }
    }

    impl Drop for OpenHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper uniquely owns the handle it opened.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn creation_time(handle: HANDLE) -> Option<u64> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: four correctly typed writable output structures and a live
        // handle with PROCESS_QUERY_LIMITED_INFORMATION.
        let queried = unsafe {
            GetProcessTimes(
                handle,
                &raw mut created,
                &raw mut exited,
                &raw mut kernel,
                &raw mut user,
            )
        };
        if queried == 0 {
            return None;
        }
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub(super) fn process_creation_time(pid: u32) -> Option<u64> {
        let handle = OpenHandle::open(pid)?;
        creation_time(handle.0)
    }

    pub(super) fn process_alive(pid: u32, expected_creation_time: u64) -> bool {
        let Some(handle) = OpenHandle::open(pid) else {
            return false;
        };
        if creation_time(handle.0) != Some(expected_creation_time) {
            // The pid was reused: whatever is running under it now is not the
            // process the caller asked about.
            return false;
        }
        // SAFETY: the handle carries SYNCHRONIZE. A process object is signaled
        // exactly when the process has terminated, which is a stronger answer
        // than an exit code a job termination chooses for us.
        unsafe { WaitForSingleObject(handle.0, 0) == WAIT_TIMEOUT }
    }

    pub(super) fn resume_only_thread(process_id: u32) -> io::Result<()> {
        let thread_handle = primary_thread(process_id)?;
        // SAFETY: this handle has THREAD_SUSPEND_RESUME access and identifies
        // the primary thread created suspended by `Command::spawn`.
        if unsafe { ResumeThread(thread_handle.0) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// How many outstanding suspends the child's primary thread carries.
    ///
    /// The Windows counterpart of `child_leads_its_own_group`: an oracle for
    /// "is this child still suspended" that asks the kernel rather than the
    /// crate, so the `CreatedSuspended`, `PrivateJobAssigned` and `Resumed`
    /// coordinates can be measured against the operations they name instead of
    /// against each other. `SuspendThread` returns the count *before* its own
    /// increment and the matching `ResumeThread` puts it back, so the
    /// observation leaves the child exactly as it found it.
    ///
    /// Test-only, like the Unix one and for the same reason: as a production
    /// guard it could only ever withhold a point it cannot add information to.
    #[cfg(test)]
    pub(super) fn primary_thread_suspend_count(process_id: u32) -> io::Result<u32> {
        use windows_sys::Win32::System::Threading::SuspendThread;

        let thread_handle = primary_thread(process_id)?;
        // SAFETY: the handle carries THREAD_SUSPEND_RESUME and names a live
        // thread; the immediately following resume restores the count.
        let previous = unsafe { SuspendThread(thread_handle.0) };
        if previous == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above; this undoes the suspend just taken.
        if unsafe { ResumeThread(thread_handle.0) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok(previous)
    }

    /// A suspend/resume handle on `process_id`'s primary thread.
    fn primary_thread(process_id: u32) -> io::Result<Snapshot> {
        // CREATE_SUSPENDED prevents the process from creating another thread,
        // so the one owned thread in this system snapshot is necessarily its
        // primary thread.
        // SAFETY: the snapshot call has no borrowed inputs.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Snapshot(snapshot);
        #[expect(clippy::expect_used, reason = "a fixed Win32 struct size fits in u32")]
        let mut entry = THREADENTRY32 {
            dwSize: u32::try_from(size_of::<THREADENTRY32>())
                .expect("thread entry structure fits in u32"),
            ..THREADENTRY32::default()
        };
        // SAFETY: `entry` advertises its correct size and remains writable.
        if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let thread_id = loop {
            if entry.th32OwnerProcessID == process_id {
                break entry.th32ThreadID;
            }
            // SAFETY: same valid snapshot and output entry as above.
            if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "could not find the suspended agent primary thread",
                ));
            }
        };
        // SAFETY: the enumerated thread id belongs to the still-suspended
        // child; the returned handle is non-inheritable.
        let thread_handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if thread_handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Snapshot(thread_handle))
    }

    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            // SAFETY: this wrapper uniquely owns its snapshot/thread handle.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// `AssignProcessToJobObject` answers with a Win32 `BOOL`: **zero is
        /// failure and every other value is success**, `-1` included.
        ///
        /// Every real assignment on a working machine succeeds, so the branch
        /// that reads this value is unreachable in an ordinary test and
        /// `if joined == 0` could become `if joined == -1` with the suite
        /// green — while an actual refusal (an outer job with UI restrictions,
        /// a job the process may not join) was read as success and startup
        /// returned `Ok` holding an ambient job with no members. The
        /// coordinator would then take workspace effects and spawn children
        /// that no ambient job owns, which is the whole of INV-18's host
        /// portion.
        ///
        /// The expected mapping is Win32's, written here, not read from the
        /// code under test.
        #[test]
        fn the_ambient_join_reads_a_win32_bool_the_way_win32_defines_one() {
            let refused =
                create_ambient(|_, _| 0).expect_err("a zero BOOL is a refused assignment");
            assert!(
                refused.contains("could not join"),
                "the diagnostic must name the join: {refused}"
            );

            // Every other value is success. Each of these creates a real job
            // object this process is deliberately *not* a member of; the
            // handle is left open exactly as the real ambient one is, and a
            // kill-on-close job with no members terminates nothing.
            for value in [1_i32, -1, i32::MIN, i32::MAX] {
                let job = create_ambient(move |_, _| value)
                    .unwrap_or_else(|error| panic!("BOOL {value} is success, not: {error}"));
                assert!(!job.0.is_null(), "BOOL {value} produced no job handle");
            }
        }

        /// The other two thirds of the sentence the join test covers.
        ///
        /// `expected_failures_refusals[1]` is "ambient job cannot be
        /// **created** or joined (Windows) → write command refuses at startup
        /// with a diagnostic", and INV-18's host portion is "refusal before any
        /// effect if the ambient job cannot be **established**". Establishing
        /// is three Win32 calls, not one: `CreateJobObjectW`,
        /// `SetInformationJobObject`, `AssignProcessToJobObject`. Only the last
        /// had a seam, so the first two could each have been ignored — an
        /// ambient job that was never created, or one created without
        /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and therefore with no fail-safe
        /// at all — with the suite green.
        ///
        /// Both failures are unreachable on a working machine, which is why
        /// they need the seam rather than a fixture.
        #[test]
        fn the_ambient_job_refuses_when_it_cannot_be_created_or_configured() {
            use std::cell::Cell;

            // A job that cannot be created is not configured and not joined.
            let configured = Cell::new(false);
            let refused = create_ambient_with(
                || {
                    Job::create_with(ptr::null_mut, |_, _, _| {
                        configured.set(true);
                        1
                    })
                },
                |_, _| panic!("a job that was never created must not be joined"),
            )
            .expect_err("a job that cannot be created is not an ambient job");
            assert!(
                !configured.get(),
                "an uncreated job was handed to SetInformationJobObject"
            );
            assert!(
                refused.contains("could not be created"),
                "the diagnostic must name creation: {refused}"
            );

            // A job that cannot be configured is refused, not kept: without
            // KILL_ON_JOB_CLOSE the ambient job terminates nothing on
            // coordinator death, which is the whole of INV-18's host portion.
            let refused = create_ambient_with(
                || Job::create_with(real_create_job, |_, _, _| 0),
                |_, _| panic!("an unconfigured job must not be joined"),
            )
            .expect_err("an unconfigured job is not an ambient job");
            assert!(
                refused.contains("could not be created"),
                "the diagnostic must name establishment: {refused}"
            );
        }

        /// What `SetInformationJobObject` is actually asked for.
        ///
        /// `KILL_ON_JOB_CLOSE` is the mechanism DESIGN.md:402 names — "abrupt
        /// conductor death closes its non-inheritable handle and lets the
        /// kernel terminate ordinary descendants" — and a job configured with
        /// any other limit flag would still return success. The expected flag
        /// and the expected structure size are Win32's, written here rather
        /// than read back from the call under test.
        #[test]
        fn every_job_this_module_creates_is_configured_to_kill_on_close() {
            use std::cell::Cell;

            let seen = Cell::new(None);
            let job = Job::create_with(real_create_job, |handle, limits, size| {
                seen.set(Some((limits.BasicLimitInformation.LimitFlags, size)));
                real_configure_job(handle, limits, size)
            })
            .expect("create a job the ordinary way");
            assert!(!job.handle.is_null());
            let (flags, size) = seen.get().expect("the configuration call was made");
            assert_eq!(
                flags, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                "the kill-on-close fail-safe is the limit this job exists for"
            );
            assert_eq!(
                size,
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).expect("fits"),
                "the extended limit structure is declared at its own size"
            );
        }

        /// An accounting error is an error, never an empty job.
        ///
        /// R22 releases the host process "on exit, timeout kill, cancel, or
        /// shutdown (private Job Object / process group)", and
        /// `QueryInformationJobObject` is the only thing that reports whether
        /// that release happened. Reading a failed query as zero would report a
        /// job settled while it still held a live member — the accounting
        /// saying "released" over a resource that is not.
        #[test]
        fn a_failed_accounting_query_is_never_read_as_an_empty_job() {
            let job = Job::create().expect("create a job");
            let error = job
                .active_processes_with(|_, _| 0)
                .expect_err("a zero BOOL from QueryInformationJobObject is a failure");
            assert!(
                !format!("{error}").is_empty(),
                "the OS's reason must survive"
            );
            // And a query that answers is believed, whatever it answers.
            for reported in [0_u32, 1, 7] {
                let observed = job
                    .active_processes_with(move |_, accounting| {
                        accounting.ActiveProcesses = reported;
                        1
                    })
                    .expect("a successful query is not an error");
                assert_eq!(observed, reported);
            }
        }

        /// Cleanup **observes** the job empty; it does not assume it.
        ///
        /// DESIGN.md:402 — "Direct-child success and timeout both terminate and
        /// boundedly observe that job empty". A real job empties by the first
        /// query, so an implementation that skipped the loop entirely is
        /// indistinguishable from this one on any real tree. The accounting
        /// responses here are chosen, not observed: 1, 1, 0.
        #[test]
        fn cleanup_polls_the_accounting_until_the_job_is_empty() {
            use std::cell::Cell;

            let job = Job::create().expect("create a job");
            let terminated = Cell::new(false);
            let answers = Cell::new(0_usize);
            job.terminate_and_wait_with(
                |_| {
                    terminated.set(true);
                    1
                },
                |_, accounting| {
                    let index = answers.get();
                    answers.set(index + 1);
                    accounting.ActiveProcesses = if index < 2 { 1 } else { 0 };
                    1
                },
            )
            .expect("cleanup completes once the job reports empty");
            assert!(terminated.get(), "the job was never terminated");
            assert_eq!(
                answers.get(),
                3,
                "cleanup returned before the accounting said zero, or kept asking after it did"
            );
        }

        /// And the observation is **bounded**.
        ///
        /// A job that never reports empty must produce a diagnostic within the
        /// documented two seconds rather than pinning a supervisor thread for
        /// the life of the process. The bound is asserted from outside, on a
        /// worker thread, so an unbounded loop fails this test with a named
        /// message instead of hanging the whole binary.
        #[test]
        fn cleanup_gives_up_on_a_job_that_never_empties() {
            use std::sync::mpsc;

            let (sender, receiver) = mpsc::channel();
            thread::spawn(move || {
                let job = Job::create().expect("create a job");
                let outcome = job
                    .terminate_and_wait_with(
                        |_| 1,
                        |_, accounting| {
                            accounting.ActiveProcesses = 1;
                            1
                        },
                    )
                    .map_err(|error| (error.kind(), error.to_string()));
                let _ = sender.send(outcome);
            });
            let outcome = receiver
                .recv_timeout(Duration::from_secs(30))
                .expect("cleanup must be bounded: it never returned");
            let (kind, message) = outcome.expect_err("a job that never empties is not settled");
            assert_eq!(kind, io::ErrorKind::TimedOut, "{message}");
            assert!(
                message.contains("2 seconds"),
                "the diagnostic must name its bound: {message}"
            );
        }
    }
}

#[cfg(unix)]
fn child_exited_unreaped(child: &Child) -> std::io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let pid = libc::id_t::try_from(child.id())
        .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { info.si_pid() } == 0 {
        return Ok(false);
    }
    // WEXITED should filter non-terminal transitions, but Darwin can leave a
    // stopped/continued record observable around job-control delivery. Never
    // turn such a record into permission for the reaper to SIGKILL the group.
    Ok(matches!(
        info.si_code,
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
    ))
}

/// Process-wide Unix termination coordination.
///
/// A signal handler may only perform async-signal-safe work. It therefore
/// stores the first terminating signal in an atomic and returns. A detached
/// monitor thread owns the locks, termination, and job-control forwarding,
/// then restores the default disposition and re-raises a terminating signal.
/// `spawning` closes the otherwise unavoidable race between `Command::spawn`
/// and pid registration; the monitor cannot terminate or suspend the parent
/// while it is nonzero.
#[cfg(unix)]
mod termination {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use crate::error::UpstrokeError;
    use crate::topology::effects::ProcessSite;

    static PENDING_TERMINATION: AtomicI32 = AtomicI32::new(0);
    static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);
    static CONTINUE_REQUESTED: AtomicBool = AtomicBool::new(false);
    static SUSPEND_ARMED: AtomicBool = AtomicBool::new(false);
    static GUARD_COMMAND_FD: AtomicI32 = AtomicI32::new(-1);
    static GUARD_WAKE_FD: AtomicI32 = AtomicI32::new(-1);
    static PROBE_PID: AtomicI32 = AtomicI32::new(-1);
    static HANDLED_TERMINATION_MASK: AtomicU8 = AtomicU8::new(0);
    static STATE: OnceLock<Result<Arc<Mutex<State>>, String>> = OnceLock::new();

    const GUARD_READY: u8 = 0x91;
    const GUARD_ARM: u8 = 0xa1;
    const GUARD_STOP: u8 = 0xb1;
    const GUARD_STOPPED: u8 = 0xb2;
    const GUARD_CANCELLED: u8 = 0xc1;
    const GUARD_DISARM: u8 = 0xd1;
    const GUARD_PROBE: u8 = 0xe1;
    const HANDLE_SIGINT: u8 = 1 << 0;
    const HANDLE_SIGTERM: u8 = 1 << 1;
    const HANDLE_SIGHUP: u8 = 1 << 2;
    const HANDLE_SIGQUIT: u8 = 1 << 3;
    const HANDLE_SIGTSTP: u8 = 1 << 0;
    const HANDLE_SIGTTIN: u8 = 1 << 1;
    const HANDLE_SIGTTOU: u8 = 1 << 2;
    const REAPER_READY: u8 = 0x81;
    const REAPER_REGISTER: u8 = 0x82;
    const REAPER_CLEANUP: u8 = 0x83;
    const REAPER_OK: u8 = 0x84;
    const REAPER_FAIL: u8 = 0x85;
    const REAPER_CANCEL: u8 = 0x86;
    // The job-control guard briefly continues only Upstroke every 250 ms while
    // probing for a PID-directed termination. The cleanup reaper must not
    // mistake that internal pulse for an operator resume and continue agents.
    // Genuine SIGCONT is forwarded immediately by the monitor; this bounded
    // fallback exists for host-owned signal policies the monitor preserves.
    const REAPER_RESUME_STABLE_POLLS: u8 = 50;

    #[derive(Clone, Copy)]
    struct SignalPolicy {
        termination_mask: u8,
        guard_wake_mask: u8,
        stop_mask: u8,
        job_control: bool,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SignalDisposition {
        Default,
        Ignored,
        Custom,
    }

    impl SignalPolicy {
        fn handles_termination(self, signal: libc::c_int) -> bool {
            let bit = match signal {
                libc::SIGINT => HANDLE_SIGINT,
                libc::SIGTERM => HANDLE_SIGTERM,
                libc::SIGHUP => HANDLE_SIGHUP,
                libc::SIGQUIT => HANDLE_SIGQUIT,
                _ => return false,
            };
            self.termination_mask & bit != 0
        }

        fn wakes_guard(self, signal: libc::c_int) -> bool {
            let bit = match signal {
                libc::SIGINT => HANDLE_SIGINT,
                libc::SIGTERM => HANDLE_SIGTERM,
                libc::SIGHUP => HANDLE_SIGHUP,
                libc::SIGQUIT => HANDLE_SIGQUIT,
                _ => return false,
            };
            self.guard_wake_mask & bit != 0
        }

        fn handles_stop(self, signal: libc::c_int) -> bool {
            let bit = match signal {
                libc::SIGTSTP => HANDLE_SIGTSTP,
                libc::SIGTTIN => HANDLE_SIGTTIN,
                libc::SIGTTOU => HANDLE_SIGTTOU,
                _ => return false,
            };
            self.stop_mask & bit != 0
        }
    }

    struct State {
        /// Supervisors that entered before spawn but have not registered a pid.
        spawning: usize,
        /// Active isolated process groups. A signal lease pins the numeric
        /// identity until the monitor has delivered its snapshot's signal, so
        /// `finish` cannot reap the leader and expose that id for reuse first.
        groups: Vec<RegisteredGroup>,
        /// Set by the monitor before it kills groups. No later spawn may begin.
        terminating: bool,
        /// Set before a suspend snapshot and cleared only after continuation.
        /// New launches wait outside the lock for the complete transition.
        suspending: bool,
        guard: Guard,
    }

    struct RegisteredGroup {
        pgid: i32,
        signal_leases: usize,
    }

    struct GroupSnapshot {
        state: Arc<Mutex<State>>,
        pgids: Vec<i32>,
    }

    impl std::ops::Deref for GroupSnapshot {
        type Target = [i32];

        fn deref(&self) -> &Self::Target {
            &self.pgids
        }
    }

    impl Drop for GroupSnapshot {
        fn drop(&mut self) {
            let mut locked = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for pgid in &self.pgids {
                if let Some(group) = locked.groups.iter_mut().find(|group| group.pgid == *pgid) {
                    group.signal_leases = group.signal_leases.saturating_sub(1);
                }
            }
        }
    }

    #[derive(Clone, Copy)]
    struct Reaper {
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        _command_keepalive_fd: libc::c_int,
        pid: libc::pid_t,
    }

    #[derive(Clone, Copy)]
    struct Guard {
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        // Keep one parent-side reader open so a guard crash turns the next arm
        // into an acknowledgement EOF instead of delivering SIGPIPE from an
        // async signal handler that writes the command pipe.
        _command_keepalive_fd: libc::c_int,
        pid: libc::pid_t,
    }

    enum Phase {
        Spawning,
        Group(i32),
        Finished,
    }

    pub(super) struct Supervisor {
        state: Arc<Mutex<State>>,
        phase: Phase,
        reaper: Reaper,
        terminate_site: ProcessSite,
    }

    impl Supervisor {
        pub(super) fn begin(terminate_site: ProcessSite) -> Result<Self, UpstrokeError> {
            if terminate_site != ProcessSite::Terminate {
                return Err(UpstrokeError::Agent {
                    message: format!(
                        "process termination requires Process.Terminate, got {}",
                        terminate_site.name()
                    ),
                });
            }
            Self::begin_with_state(shared_state()?, terminate_site)
        }

        fn begin_with_state(
            state: Arc<Mutex<State>>,
            terminate_site: ProcessSite,
        ) -> Result<Self, UpstrokeError> {
            claim_launch(&state)?;
            let reaper = match spawn_reaper() {
                Ok(reaper) => reaper,
                Err(message) => {
                    release_launch(&state);
                    return Err(UpstrokeError::Agent { message });
                }
            };
            Ok(Self {
                state,
                phase: Phase::Spawning,
                reaper,
                terminate_site,
            })
        }

        pub(super) fn prepare(&self, command: &mut std::process::Command) {
            use std::os::unix::process::CommandExt;

            let reaper = self.reaper;
            // SAFETY: the closure uses only async-signal-safe syscalls. It
            // creates the private process group and registers it with the
            // external reaper before exec, so even SIGKILL in the parent's
            // post-spawn registration window cannot orphan the agent tree.
            unsafe {
                command.pre_exec(move || {
                    let pid = libc::getpid();
                    if libc::setpgid(0, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if reaper.register_raw(pid) {
                        Ok(())
                    } else {
                        Err(std::io::Error::from_raw_os_error(libc::EIO))
                    }
                });
            }
        }

        pub(super) fn register(&mut self, pid: u32) -> Result<(), UpstrokeError> {
            let pgid = i32::try_from(pid).map_err(|_| UpstrokeError::Agent {
                message: format!("agent pid {pid} cannot be represented as a Unix process group"),
            })?;
            let mut locked = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locked.spawning = locked.spawning.saturating_sub(1);
            locked.groups.push(RegisteredGroup {
                pgid,
                signal_leases: 0,
            });
            self.phase = Phase::Group(pgid);
            Ok(())
        }

        pub(super) fn finish(&mut self) -> Result<(), UpstrokeError> {
            if self.terminate_site != ProcessSite::Terminate {
                return Err(UpstrokeError::Agent {
                    message: format!(
                        "process termination requires Process.Terminate, got {}",
                        self.terminate_site.name()
                    ),
                });
            }
            let Phase::Group(pgid) = self.phase else {
                return Ok(());
            };
            // `cleanup` consumes and closes the reaper's raw descriptors.
            // Change phase first so an error return followed by Drop can never
            // transact on—or close—descriptor numbers another thread may
            // already have reused.
            self.phase = Phase::Finished;
            if !self.reaper.cleanup(pgid) {
                arm_fail_closed_termination(
                    b"upstroke: fail-closed SIGTERM armed: cleanup reaper did not acknowledge CLEANUP\n",
                );
                return Err(UpstrokeError::Agent {
                    message: format!(
                        "Unix cleanup reaper failed while settling process group {pgid}"
                    ),
                });
            }
            loop {
                let mut locked = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if !locked.groups.iter().any(|group| group.pgid == pgid) {
                    return Ok(());
                }
                if remove_unpinned_group(&mut locked, pgid) {
                    return Ok(());
                }
                drop(locked);
                thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn claim_launch(state: &Arc<Mutex<State>>) -> Result<(), UpstrokeError> {
        loop {
            let mut locked = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if locked.terminating || PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {
                return Err(UpstrokeError::Agent {
                    message: "process launch interrupted by a termination signal".to_owned(),
                });
            }
            if !locked.suspending && locked.spawning == 0 {
                locked.spawning = locked.spawning.saturating_add(1);
                return Ok(());
            }
            drop(locked);
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// The process groups the parent supervisor currently has registered.
    ///
    /// Test-only, and an oracle rather than a guard: `Spawn.Registered` is
    /// "parent-side registration", and the only way to ask whether that
    /// happened before the point fired is to read the state it writes.
    #[cfg(test)]
    pub(super) fn registered_groups() -> Vec<i32> {
        let Ok(state) = shared_state() else {
            return Vec::new();
        };
        let locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locked.groups.iter().map(|group| group.pgid).collect()
    }

    fn release_launch(state: &Arc<Mutex<State>>) {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locked.spawning = locked.spawning.saturating_sub(1);
    }

    impl Drop for Supervisor {
        fn drop(&mut self) {
            match self.phase {
                Phase::Spawning => {
                    self.reaper.cancel();
                    release_launch(&self.state);
                }
                Phase::Group(_) => {
                    // `finish` normally runs while the direct child is still
                    // an unreaped zombie, pinning the process-group identity.
                    // Error unwinding remains fail-closed through the same
                    // external reaper rather than trusting a recycled PGID;
                    // a failure consumes the reaper and arms process exit.
                    let _ = self.finish();
                }
                Phase::Finished => {}
            }
        }
    }

    fn shared_state() -> Result<Arc<Mutex<State>>, UpstrokeError> {
        match STATE.get_or_init(install) {
            Ok(state) => Ok(Arc::clone(state)),
            Err(message) => Err(UpstrokeError::Agent {
                message: message.clone(),
            }),
        }
    }

    fn install() -> Result<Arc<Mutex<State>>, String> {
        let mut policy = SignalPolicy {
            termination_mask: 0,
            guard_wake_mask: 0,
            stop_mask: 0,
            job_control: false,
        };
        for (signal, bit) in [
            (libc::SIGINT, HANDLE_SIGINT),
            (libc::SIGTERM, HANDLE_SIGTERM),
            (libc::SIGHUP, HANDLE_SIGHUP),
            (libc::SIGQUIT, HANDLE_SIGQUIT),
        ] {
            match disposition(signal)? {
                SignalDisposition::Default => {
                    policy.termination_mask |= bit;
                    policy.guard_wake_mask |= bit;
                }
                SignalDisposition::Custom => policy.guard_wake_mask |= bit,
                SignalDisposition::Ignored => {}
            }
        }
        for (signal, bit) in [
            (libc::SIGTSTP, HANDLE_SIGTSTP),
            (libc::SIGTTIN, HANDLE_SIGTTIN),
            (libc::SIGTTOU, HANDLE_SIGTTOU),
        ] {
            if disposition(signal)? == SignalDisposition::Default {
                policy.stop_mask |= bit;
            }
        }
        let continue_disposition = disposition(libc::SIGCONT)?;
        if policy.stop_mask != 0 && continue_disposition != SignalDisposition::Default {
            return Err(
                "cannot safely proxy default Unix job-control stops while the embedding host owns or ignores SIGCONT"
                    .to_owned(),
            );
        }
        policy.job_control = policy.stop_mask != 0;
        HANDLED_TERMINATION_MASK.store(policy.termination_mask, Ordering::SeqCst);

        let guard = spawn_guard(policy)?;
        let state = Arc::new(Mutex::new(State {
            spawning: 0,
            groups: Vec::new(),
            terminating: false,
            suspending: false,
            guard,
        }));
        let monitored = Arc::clone(&state);
        let (monitor_ready, monitor_started) = std::sync::mpsc::sync_channel(1);
        let monitor = thread::Builder::new()
            .name("upstroke-signal-monitor".to_owned())
            .spawn(move || match prepare_monitor_signal_mask(policy) {
                Ok(()) => {
                    let _ = monitor_ready.send(Ok(()));
                    monitor(monitored)
                }
                Err(error) => {
                    let _ = monitor_ready.send(Err(error));
                }
            });
        let monitor = match monitor {
            Ok(monitor) => monitor,
            Err(error) => {
                guard.abort_setup();
                return Err(format!("starting Unix signal monitor: {error}"));
            }
        };
        match monitor_started.recv() {
            Ok(Ok(())) => drop(monitor),
            Ok(Err(error)) => {
                let _ = monitor.join();
                guard.abort_setup();
                return Err(error);
            }
            Err(error) => {
                let _ = monitor.join();
                guard.abort_setup();
                return Err(format!(
                    "starting Unix signal monitor: readiness channel closed: {error}"
                ));
            }
        }

        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            // Preserve every launcher-owned policy. POSIX carries SIG_IGN
            // across exec (`nohup` relies on it), while an embedding host may
            // have installed a custom in-process handler before calling us.
            if policy.handles_termination(signal) {
                install_handler(signal)?;
            }
        }

        // Preserve every host-owned stop disposition. Each remaining default
        // terminal stop is proxied, and the policy check above guarantees the
        // matching default SIGCONT can release the isolated groups again.
        if policy.job_control {
            for signal in [libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU] {
                if policy.handles_stop(signal) {
                    install_handler(signal)?;
                }
            }
            install_handler(libc::SIGCONT)?;
        }
        Ok(state)
    }

    fn disposition(signal: libc::c_int) -> Result<SignalDisposition, String> {
        // SAFETY: a null `act` queries the current disposition without
        // changing it; `previous` is initialized by a successful call.
        unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(signal, std::ptr::null(), &mut previous) != 0 {
                return Err(format!(
                    "reading Unix signal disposition for signal {signal}: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(if previous.sa_sigaction == libc::SIG_IGN {
                SignalDisposition::Ignored
            } else if previous.sa_sigaction == libc::SIG_DFL {
                SignalDisposition::Default
            } else {
                SignalDisposition::Custom
            })
        }
    }

    fn prepare_monitor_signal_mask(policy: SignalPolicy) -> Result<(), String> {
        if !policy.job_control {
            return Ok(());
        }

        // An embedding host may have blocked SIGCONT on the thread that first
        // called Upstroke, and new threads inherit that mask. SIGCONT still wakes
        // a stopped process when blocked, but its handler cannot run, so the
        // isolated agent groups would remain stopped forever. Give only the
        // private monitor thread an unblocked SIGCONT; every host thread keeps
        // its original mask.
        unsafe {
            let mut signals: libc::sigset_t = std::mem::zeroed();
            if libc::sigemptyset(&mut signals) != 0
                || libc::sigaddset(&mut signals, libc::SIGCONT) != 0
            {
                return Err(format!(
                    "building Unix signal-monitor mask: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let result = libc::pthread_sigmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut());
            if result != 0 {
                return Err(format!(
                    "unblocking SIGCONT in Unix signal monitor: {}",
                    std::io::Error::from_raw_os_error(result)
                ));
            }
        }
        Ok(())
    }

    fn install_handler(signal: libc::c_int) -> Result<(), String> {
        // SAFETY: `record_signal` has the C ABI and performs only lock-free
        // atomic operations. The empty mask and SA_RESTART keep unrelated
        // syscalls from being exposed to the implementation detail that a
        // monitor thread, rather than the handler, owns process-group work.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            if signal == libc::SIGCONT {
                action.sa_sigaction = record_signal_info as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
            } else {
                action.sa_sigaction = record_signal as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART;
            }
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(format!(
                    "installing Unix signal forwarding for signal {signal}: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    }

    extern "C" fn record_signal(signal: libc::c_int) {
        match signal {
            libc::SIGTSTP | libc::SIGTTIN | libc::SIGTTOU => {
                SUSPEND_REQUESTED.store(true, Ordering::SeqCst)
            }
            libc::SIGCONT => {
                CONTINUE_REQUESTED.store(true, Ordering::SeqCst);
                notify_guard(signal);
            }
            _ => {
                let _ = PENDING_TERMINATION.compare_exchange(
                    0,
                    signal,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                notify_guard(signal);
            }
        }
    }

    extern "C" fn record_signal_info(
        signal: libc::c_int,
        info: *mut libc::siginfo_t,
        _: *mut libc::c_void,
    ) {
        let is_guard_probe = signal == libc::SIGCONT
            && !info.is_null()
            && unsafe { (*info).si_pid() } == PROBE_PID.load(Ordering::SeqCst);
        if !is_guard_probe {
            record_signal(signal);
            return;
        }

        // A stopped process cannot execute a caught termination handler. The
        // external guard periodically resumes only Upstroke so this handler can
        // inspect/deliver a PID-directed pending signal; supervised agent
        // groups remain stopped. With no such signal, stop again from inside
        // the handler before returning to ordinary parent code.
        let already_recorded = PENDING_TERMINATION.load(Ordering::SeqCst);
        if already_recorded != 0 {
            notify_guard(already_recorded);
            return;
        }
        let pending = pending_termination_signal();
        if pending != 0 {
            let _ = PENDING_TERMINATION.compare_exchange(
                0,
                pending,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            notify_guard(pending);
            return;
        }
        if unsafe { libc::kill(libc::getpid(), libc::SIGSTOP) } != 0 {
            arm_fail_closed_termination(
                b"upstroke: fail-closed SIGTERM armed: self-stop failed after a guard probe\n",
            );
            notify_guard(libc::SIGTERM);
        }
    }

    fn pending_termination_signal() -> libc::c_int {
        unsafe {
            let mut pending: libc::sigset_t = std::mem::zeroed();
            if libc::sigpending(&mut pending) != 0 {
                return libc::SIGTERM;
            }
            let mask = HANDLED_TERMINATION_MASK.load(Ordering::SeqCst);
            for (signal, bit) in [
                (libc::SIGINT, HANDLE_SIGINT),
                (libc::SIGTERM, HANDLE_SIGTERM),
                (libc::SIGHUP, HANDLE_SIGHUP),
                (libc::SIGQUIT, HANDLE_SIGQUIT),
            ] {
                if mask & bit != 0 && libc::sigismember(&pending, signal) == 1 {
                    return signal;
                }
            }
        }
        0
    }

    extern "C" fn record_guard_signal(signal: libc::c_int) {
        let fd = GUARD_WAKE_FD.load(Ordering::SeqCst);
        if fd < 0 {
            return;
        }
        let byte = u8::try_from(signal).unwrap_or(u8::MAX);
        // SAFETY: the wake descriptor is nonblocking and dedicated to the
        // guard's self-pipe. A full pipe is already readable, so dropping that
        // byte cannot lose the wakeup.
        let _ = unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }

    fn notify_guard(signal: libc::c_int) {
        if !SUSPEND_ARMED.load(Ordering::SeqCst) {
            return;
        }
        let fd = GUARD_COMMAND_FD.load(Ordering::SeqCst);
        if fd < 0 {
            return;
        }
        let byte = u8::try_from(signal).unwrap_or(u8::MAX);
        // SAFETY: `write` is async-signal-safe, the descriptor is a dedicated
        // pipe, and a one-byte record is atomic. The parent retains a reader so
        // a failed guard cannot turn this write into SIGPIPE.
        let _ = unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }

    /// Arm fail-closed termination with `SIGTERM`, and say so on fd 2.
    ///
    /// Every fallback that gives up on a private helper comes through here so
    /// the process about to die names the site first. `C-004`: the macOS test
    /// harness SIGTERMed itself for a day without a diagnostic — libtest
    /// captures the failing test's panic and the raise pre-empts its report —
    /// and four CI deaths were read backwards as a group-kill that never
    /// happened. One raw `write` survives both, and it is async-signal-safe,
    /// which the guard-probe handler needs. Only the site that wins the arm
    /// writes; a later fallback that finds a termination already pending is
    /// not the cause and says nothing.
    fn arm_fail_closed_termination(site: &[u8]) {
        if PENDING_TERMINATION
            .compare_exchange(0, libc::SIGTERM, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let _ = write_raw(libc::STDERR_FILENO, site);
        }
    }

    fn monitor(state: Arc<Mutex<State>>) -> ! {
        loop {
            let terminating = PENDING_TERMINATION.load(Ordering::SeqCst);
            if terminating != 0 {
                let Some(groups) = groups_when_registered(&state, true) else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                signal_groups(&groups, libc::SIGKILL);

                // SAFETY: all isolated children have been synchronously sent
                // SIGKILL. Restore the ordinary terminal semantics and
                // terminate Upstroke with the original signal; `_exit` is a
                // defensive fallback if a platform returns from `raise`.
                unsafe {
                    libc::signal(terminating, libc::SIG_DFL);
                    libc::raise(terminating);
                    libc::_exit(128 + terminating);
                }
            }

            if SUSPEND_REQUESTED.swap(false, Ordering::SeqCst) {
                let Some((groups, guard)) = begin_suspend(&state) else {
                    SUSPEND_REQUESTED.store(true, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                // SIGSTOP cannot be caught or ignored, so a vendor process
                // cannot keep spending while its visibly foreground Upstroke
                // parent is suspended. SIGCONT below releases the same groups.
                if !stop_groups(&groups) {
                    let groups = end_suspend(&state);
                    if PENDING_TERMINATION.load(Ordering::SeqCst) == 0 {
                        signal_groups(&groups, libc::SIGCONT);
                    }
                    continue;
                }

                // The guard remains runnable while Upstroke is stopped. It
                // serializes a late continuation/termination with the actual
                // SIGSTOP and acknowledges only after a genuine resume. That
                // closes the final flag-check-to-stop interval.
                SUSPEND_ARMED.store(true, Ordering::SeqCst);
                if !guard.arm() {
                    SUSPEND_ARMED.store(false, Ordering::SeqCst);
                    let _ = end_suspend(&state);
                    arm_fail_closed_termination(
                        b"upstroke: fail-closed SIGTERM armed: job-control guard did not acknowledge ARM\n",
                    );
                    continue;
                }

                // A terminating signal wins over suspension. Do not stop the
                // parent after its monitor has already been asked to tear down.
                if PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {
                    SUSPEND_ARMED.store(false, Ordering::SeqCst);
                    guard.disarm();
                    let _ = end_suspend(&state);
                    continue;
                }
                if CONTINUE_REQUESTED.swap(false, Ordering::SeqCst) {
                    SUSPEND_ARMED.store(false, Ordering::SeqCst);
                    guard.disarm();
                    let groups = end_suspend(&state);
                    signal_groups(&groups, libc::SIGCONT);
                    continue;
                }

                // The external guard sends SIGSTOP while this monitor is
                // blocked on its acknowledgement pipe. Therefore the next
                // instruction cannot run until a real later SIGCONT; queuing a
                // self-signal here would allow cleanup to race ahead of the
                // kernel's process-wide stop.
                match guard.stop_parent() {
                    Some(true) => {}
                    Some(false) => {
                        SUSPEND_ARMED.store(false, Ordering::SeqCst);
                        guard.disarm();
                        let groups = end_suspend(&state);
                        if PENDING_TERMINATION.load(Ordering::SeqCst) == 0 {
                            signal_groups(&groups, libc::SIGCONT);
                        }
                        continue;
                    }
                    None => {
                        SUSPEND_ARMED.store(false, Ordering::SeqCst);
                        guard.disarm();
                        let _ = end_suspend(&state);
                        arm_fail_closed_termination(
                            b"upstroke: fail-closed SIGTERM armed: job-control guard lost the STOP handshake\n",
                        );
                        continue;
                    }
                }
                SUSPEND_ARMED.store(false, Ordering::SeqCst);
                guard.disarm();
                let groups = end_suspend(&state);
                let terminating = PENDING_TERMINATION.load(Ordering::SeqCst) != 0;
                let _ = CONTINUE_REQUESTED.swap(false, Ordering::SeqCst);
                if !terminating {
                    signal_groups(&groups, libc::SIGCONT);
                }
                continue;
            }

            if CONTINUE_REQUESTED.swap(false, Ordering::SeqCst) {
                if let Some(groups) = groups_when_registered(&state, false) {
                    signal_groups(&groups, libc::SIGCONT);
                } else {
                    CONTINUE_REQUESTED.store(true, Ordering::SeqCst);
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn groups_when_registered(
        state: &Arc<Mutex<State>>,
        terminating: bool,
    ) -> Option<GroupSnapshot> {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if locked.spawning != 0 {
            return None;
        }
        if terminating {
            locked.terminating = true;
        }
        Some(snapshot_groups(state, &mut locked))
    }

    fn begin_suspend(state: &Arc<Mutex<State>>) -> Option<(GroupSnapshot, Guard)> {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if locked.spawning != 0 || locked.suspending || locked.terminating {
            return None;
        }
        locked.suspending = true;
        let guard = locked.guard;
        Some((snapshot_groups(state, &mut locked), guard))
    }

    fn end_suspend(state: &Arc<Mutex<State>>) -> GroupSnapshot {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locked.suspending = false;
        snapshot_groups(state, &mut locked)
    }

    fn snapshot_groups(state: &Arc<Mutex<State>>, locked: &mut State) -> GroupSnapshot {
        let mut pgids = Vec::with_capacity(locked.groups.len());
        for group in &mut locked.groups {
            group.signal_leases = group.signal_leases.saturating_add(1);
            pgids.push(group.pgid);
        }
        GroupSnapshot {
            state: Arc::clone(state),
            pgids,
        }
    }

    fn remove_unpinned_group(state: &mut State, pgid: i32) -> bool {
        let Some(index) = state.groups.iter().position(|group| group.pgid == pgid) else {
            return true;
        };
        if state.groups[index].signal_leases != 0 {
            return false;
        }
        state.groups.swap_remove(index);
        true
    }

    impl Reaper {
        /// Register from `Command::pre_exec`, where allocation and Rust locks
        /// are forbidden. Launches are serialized, so the shared one-byte
        /// acknowledgement belongs to this registration frame.
        fn register_raw(self, pgid: libc::pid_t) -> bool {
            self.transact_raw(REAPER_REGISTER, pgid) == Some(REAPER_OK)
        }

        fn cleanup(self, pgid: libc::pid_t) -> bool {
            let cleaned = self.transact_raw(REAPER_CLEANUP, pgid) == Some(REAPER_OK);
            self.close_and_wait();
            cleaned
        }

        fn transact_raw(self, operation: u8, pgid: libc::pid_t) -> Option<u8> {
            let mut frame = [0_u8; 5];
            frame[0] = operation;
            frame[1..].copy_from_slice(&pgid.to_ne_bytes());
            if !write_raw(self.command_fd, &frame) {
                return None;
            }
            read_raw_byte(self.ack_fd)
        }

        fn cancel(self) {
            let mut frame = [0_u8; 5];
            frame[0] = REAPER_CANCEL;
            let cancelled = write_raw(self.command_fd, &frame)
                && acknowledged(self.ack_fd, REAPER_OK, Duration::from_secs(2));
            if !cancelled {
                // The parent does not know whether pre_exec registered a group
                // before spawn failed. Arm ordinary fail-closed termination;
                // the independently polling reaper will observe reparenting
                // and complete any registered cleanup without trusting EOF.
                arm_fail_closed_termination(
                    b"upstroke: fail-closed SIGTERM armed: cleanup reaper did not acknowledge CANCEL\n",
                );
                close_fd(self.command_fd);
                close_fd(self.ack_fd);
                close_fd(self._command_keepalive_fd);
                return;
            }
            self.close_and_wait();
        }

        /// Give up on a reaper that has not said READY.
        ///
        /// No agent has been spawned and no group registered — `prepare` runs
        /// only after `begin` has returned — so there is nothing for this
        /// reaper to settle and nothing to fail closed about. Kill it and reap
        /// it; the launch fails with an ordinary error. Arming process-wide
        /// `SIGTERM` here guarded a state that cannot exist yet, and on macOS,
        /// where a forked helper's startup runs long under load, it killed the
        /// test harness with no diagnostic (`C-004`).
        fn abandon(self) {
            // SAFETY: `pid` is the unreaped reaper this process forked. It is
            // the only member of its own process group and holds nothing but
            // its shared cleanup lease, which its exit releases.
            unsafe {
                let _ = libc::kill(self.pid, libc::SIGKILL);
            }
            self.close_and_wait();
        }

        fn close_and_wait(self) {
            close_fd(self.command_fd);
            close_fd(self.ack_fd);
            close_fd(self._command_keepalive_fd);
            loop {
                let waited = unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
                if waited == self.pid || (waited < 0 && !last_errno_is_interrupted()) {
                    return;
                }
            }
        }
    }

    fn spawn_reaper() -> Result<Reaper, String> {
        use std::os::unix::ffi::OsStrExt;

        verify_group_scanner()?;
        let parent = unsafe { libc::getpid() };
        let cleanup_paths = crate::rundir::active_cleanup_lease_paths()
            .into_iter()
            .map(|path| {
                std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
                    format!(
                        "run cleanup-lease path contains a null byte: {}",
                        path.display()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(test)]
        let cleanup_delay_ms = std::env::var("UPSTROKE_TEST_CLEANUP_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        #[cfg(not(test))]
        let cleanup_delay_ms = 0;
        #[cfg(test)]
        let ready_delay_ms = std::env::var("UPSTROKE_TEST_REAPER_READY_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        #[cfg(not(test))]
        let ready_delay_ms = 0_u64;
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if open_max <= 0 {
            return Err("reading the Unix open-file descriptor ceiling".to_owned());
        }
        let open_max = libc::c_int::try_from(open_max)
            .map_err(|_| "Unix open-file descriptor ceiling exceeds c_int".to_owned())?;
        // Rendered BEFORE the fork, like `cleanup_paths` above and for the same
        // reason: the reaper may not allocate. `None` is the ordinary state of
        // every run today — nothing selects a container Runner until PR12 — and
        // costs the reaper nothing at all.
        let containers = container_scope_for_a_new_reaper();
        let command = create_cloexec_pipe()
            .map_err(|error| format!("creating Unix cleanup-reaper command pipe: {error}"))?;
        let ack = match create_cloexec_pipe() {
            Ok(pipe) => pipe,
            Err(error) => {
                close_fd(command[0]);
                close_fd(command[1]);
                return Err(format!(
                    "creating Unix cleanup-reaper acknowledgement pipe: {error}"
                ));
            }
        };
        // SAFETY: the child immediately enters a fixed-storage syscall-only
        // loop. It never returns to the multithreaded Rust runtime.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            for fd in [command[0], command[1], ack[0], ack[1]] {
                close_fd(fd);
            }
            return Err(format!(
                "starting Unix cleanup reaper: {}",
                std::io::Error::last_os_error()
            ));
        }
        if pid == 0 {
            if !install_reaper_dispositions() {
                unsafe { libc::_exit(1) };
            }
            close_fd(command[1]);
            close_fd(ack[0]);
            // A separate process group is the crucial boundary: an
            // uncatchable kill of Upstroke's foreground job must not also kill
            // the process that owns its final agent cleanup.
            if unsafe { libc::setpgid(0, 0) } != 0 {
                unsafe { libc::_exit(1) };
            }
            close_inherited_fds(&[command[0], ack[1]], open_max);
            if !lock_cleanup_paths(&cleanup_paths) {
                unsafe { libc::_exit(1) };
            }
            // Test subprocesses can hold READY back past the parent's deadline
            // so the late-reaper path is driven deterministically.
            let mut delay_left = ready_delay_ms;
            while delay_left > 0 {
                raw_sleep_10ms();
                delay_left = delay_left.saturating_sub(10);
            }
            reaper_loop(
                parent,
                command[0],
                ack[1],
                open_max,
                cleanup_delay_ms,
                containers.as_ref(),
            );
        }

        // Close the parent's race with the child-side setpgid. Either call may
        // win; both establish the same private group before any agent exists.
        if unsafe { libc::setpgid(pid, pid) } != 0 {
            let error = last_errno();
            if error != libc::EACCES && error != libc::EPERM {
                for fd in [command[0], command[1], ack[0], ack[1]] {
                    close_fd(fd);
                }
                unsafe {
                    let _ = libc::kill(pid, libc::SIGKILL);
                    let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
                return Err(format!(
                    "isolating Unix cleanup reaper: {}",
                    std::io::Error::from_raw_os_error(error)
                ));
            }
        }
        close_fd(ack[1]);
        let reaper = Reaper {
            command_fd: command[1],
            ack_fd: ack[0],
            _command_keepalive_fd: command[0],
            pid,
        };
        if read_guard_ack(ack[0], Duration::from_secs(2)) != Some(REAPER_READY) {
            reaper.abandon();
            return Err("Unix cleanup reaper did not initialize".to_owned());
        }
        #[cfg(test)]
        if let Some(path) = std::env::var_os("UPSTROKE_TEST_REAPER_PID_PATH") {
            if let Err(error) = std::fs::write(&path, pid.to_string()) {
                reaper.cancel();
                return Err(format!(
                    "recording test cleanup-reaper pid at {}: {error}",
                    std::path::Path::new(&path).display()
                ));
            }
        }
        Ok(reaper)
    }

    fn install_reaper_dispositions() -> bool {
        unsafe {
            // This child never executes embedding-host code. Remove every
            // inherited callback before clearing its signal mask; SIGCHLD is
            // restored to default immediately below because the reaper owns
            // the stopped anchor's wait lifecycle.
            if !scrub_private_helper_dispositions() {
                return false;
            }
            // A library host may own SIGCHLD and reap children from its
            // handler. The private reaper must not inherit that callback (or
            // SA_NOCLDWAIT): either can consume the stopped anchor before the
            // reaper's blocking waitpid observes it.
            let mut child_action: libc::sigaction = std::mem::zeroed();
            child_action.sa_sigaction = libc::SIG_DFL;
            child_action.sa_flags = 0;
            if libc::sigemptyset(&mut child_action.sa_mask) != 0
                || libc::sigaction(libc::SIGCHLD, &child_action, std::ptr::null_mut()) != 0
            {
                return false;
            }
            for signal in [
                libc::SIGINT,
                libc::SIGTERM,
                libc::SIGHUP,
                libc::SIGQUIT,
                libc::SIGTSTP,
                libc::SIGCONT,
                libc::SIGPIPE,
            ] {
                if libc::signal(signal, libc::SIG_IGN) == libc::SIG_ERR {
                    return false;
                }
            }
            let mut empty: libc::sigset_t = std::mem::zeroed();
            if libc::sigemptyset(&mut empty) != 0
                || libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) != 0
            {
                return false;
            }
        }
        true
    }

    fn lock_cleanup_paths(paths: &[std::ffi::CString]) -> bool {
        for path in paths {
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            if fd < 0 || unsafe { libc::flock(fd, libc::LOCK_SH | libc::LOCK_NB) } != 0 {
                if fd >= 0 {
                    close_fd(fd);
                }
                return false;
            }
            // Deliberately leave this independently opened descriptor live.
            // Process exit releases its shared lease after cleanup completes.
        }
        true
    }

    fn reaper_loop(
        parent: libc::pid_t,
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        open_max: libc::c_int,
        cleanup_delay_ms: u64,
        containers: Option<&ReaperContainers>,
    ) -> ! {
        let mut pgid = 0_i32;
        let mut anchor = 0_i32;
        let mut mirrored_parent_stop = false;
        let mut parent_running_polls = 0_u8;
        if !write_raw(ack_fd, &[REAPER_READY]) {
            unsafe { libc::_exit(1) };
        }
        loop {
            let mut command = libc::pollfd {
                fd: command_fd,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            // Poll even before registration. An exec-racing descendant may
            // retain a FIFO writer until it exits, so EOF is not a trustworthy
            // parent-liveness signal on Darwin. Reparenting is authoritative
            // and lets this fork-only helper settle independently.
            let polled = unsafe { libc::poll(&mut command, 1, 10) };
            if polled < 0 {
                if last_errno_is_interrupted() {
                    continue;
                }
                settle_after_coordinator_death(pgid, anchor, cleanup_delay_ms, containers);
                unsafe { libc::_exit(0) };
            }
            if unsafe { libc::getppid() } != parent {
                settle_after_coordinator_death(pgid, anchor, cleanup_delay_ms, containers);
                unsafe { libc::_exit(0) };
            }
            if polled > 0 && command.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                settle_after_coordinator_death(pgid, anchor, cleanup_delay_ms, containers);
                unsafe { libc::_exit(0) };
            }
            if polled > 0 && command.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                let mut frame = [0_u8; 5];
                if !read_raw_exact(command_fd, &mut frame) {
                    settle_after_coordinator_death(pgid, anchor, cleanup_delay_ms, containers);
                    unsafe { libc::_exit(0) };
                }
                let requested = i32::from_ne_bytes([frame[1], frame[2], frame[3], frame[4]]);
                let accepted = match frame[0] {
                    REAPER_REGISTER if pgid == 0 && requested > 0 => {
                        let created = spawn_group_anchor(requested, open_max);
                        if created <= 0 {
                            false
                        } else {
                            pgid = requested;
                            anchor = created;
                            true
                        }
                    }
                    REAPER_CLEANUP if requested == pgid && pgid > 0 => {
                        cleanup_reaper_group(pgid, anchor, cleanup_delay_ms);
                        let _ = write_raw(ack_fd, &[REAPER_OK]);
                        unsafe { libc::_exit(0) };
                    }
                    REAPER_CANCEL if requested == 0 => {
                        if pgid > 0 {
                            cleanup_reaper_group(pgid, anchor, cleanup_delay_ms);
                        }
                        let _ = write_raw(ack_fd, &[REAPER_OK]);
                        unsafe { libc::_exit(0) };
                    }
                    _ => false,
                };
                if !write_raw(ack_fd, &[if accepted { REAPER_OK } else { REAPER_FAIL }]) {
                    settle_after_coordinator_death(pgid, anchor, cleanup_delay_ms, containers);
                    unsafe { libc::_exit(0) };
                }
            }

            if pgid > 0 {
                match process_is_stopped(parent) {
                    Some(true) if !mirrored_parent_stop => {
                        mirrored_parent_stop = unsafe { libc::kill(-pgid, libc::SIGSTOP) } == 0;
                        parent_running_polls = 0;
                    }
                    Some(true) => parent_running_polls = 0,
                    state @ Some(false) if mirrored_parent_stop => {
                        if parent_has_stably_resumed(state, &mut parent_running_polls) {
                            let _ = unsafe { libc::kill(-pgid, libc::SIGCONT) };
                            mirrored_parent_stop = false;
                            parent_running_polls = 0;
                        }
                    }
                    Some(false) => parent_running_polls = 0,
                    None => parent_running_polls = 0,
                }
            }
        }
    }

    fn parent_has_stably_resumed(stopped: Option<bool>, running_polls: &mut u8) -> bool {
        if stopped != Some(false) {
            *running_polls = 0;
            return false;
        }
        *running_polls = running_polls.saturating_add(1);
        *running_polls >= REAPER_RESUME_STABLE_POLLS
    }

    fn spawn_group_anchor(pgid: i32, open_max: libc::c_int) -> libc::pid_t {
        let anchor = unsafe { libc::fork() };
        if anchor < 0 {
            return -1;
        }
        if anchor == 0 {
            if unsafe { libc::setpgid(0, pgid) } != 0 {
                unsafe { libc::_exit(1) };
            }
            close_inherited_fds(&[], open_max);
            unsafe {
                libc::raise(libc::SIGSTOP);
                loop {
                    libc::pause();
                }
            }
        }
        let mut status = 0;
        loop {
            let waited = unsafe { libc::waitpid(anchor, &mut status, libc::WUNTRACED) };
            if waited == anchor {
                if libc::WIFSTOPPED(status) {
                    return anchor;
                }
                return -1;
            }
            if waited < 0 && !last_errno_is_interrupted() {
                return -1;
            }
        }
    }

    fn cleanup_reaper_group(pgid: i32, anchor: libc::pid_t, cleanup_delay_ms: u64) {
        // Signal the kernel-owned group identity first. Even if the platform's
        // membership scanner subsequently becomes unavailable, no owned
        // process can keep running or spending while cleanup waits fail-closed.
        unsafe {
            let _ = libc::kill(-pgid, libc::SIGKILL);
        }
        // Test subprocesses can widen the otherwise tiny post-crash window so
        // the reaper-owned cleanup lease is asserted deterministically.
        // Release builds always pass zero and pay no delay.
        let mut delay_left = cleanup_delay_ms;
        while delay_left > 0 {
            raw_sleep_10ms();
            delay_left = delay_left.saturating_sub(10);
        }
        // The stopped anchor pins the PGID until it becomes our unreaped
        // zombie. Only release the reaper-owned run-cleanup lease once every
        // member of that exact group is either gone or a non-running zombie.
        while group_has_non_zombie_members(pgid) != Some(false) {
            raw_sleep_10ms();
            unsafe {
                let _ = libc::kill(-pgid, libc::SIGKILL);
            }
        }
        if anchor > 0 {
            loop {
                let waited = unsafe { libc::waitpid(anchor, std::ptr::null_mut(), 0) };
                if waited == anchor || (waited < 0 && !last_errno_is_interrupted()) {
                    break;
                }
            }
        }
    }

    fn write_raw(fd: libc::c_int, bytes: &[u8]) -> bool {
        let mut offset = 0;
        while offset < bytes.len() {
            let written =
                unsafe { libc::write(fd, bytes.as_ptr().add(offset).cast(), bytes.len() - offset) };
            if written > 0 {
                offset += written as usize;
            } else if written < 0 && last_errno_is_interrupted() {
                continue;
            } else {
                return false;
            }
        }
        true
    }

    fn read_raw_exact(fd: libc::c_int, bytes: &mut [u8]) -> bool {
        let mut offset = 0;
        while offset < bytes.len() {
            let read = unsafe {
                libc::read(
                    fd,
                    bytes.as_mut_ptr().add(offset).cast(),
                    bytes.len() - offset,
                )
            };
            if read > 0 {
                offset += read as usize;
            } else if read < 0 && last_errno_is_interrupted() {
                continue;
            } else {
                return false;
            }
        }
        true
    }

    fn read_raw_byte(fd: libc::c_int) -> Option<u8> {
        let mut byte = 0_u8;
        read_raw_exact(fd, std::slice::from_mut(&mut byte)).then_some(byte)
    }

    impl Guard {
        fn abort_setup(self) {
            let _ = GUARD_COMMAND_FD.compare_exchange(
                self.command_fd,
                -1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            PROBE_PID.store(-1, Ordering::SeqCst);
            for fd in [self.command_fd, self.ack_fd, self._command_keepalive_fd] {
                close_fd(fd);
            }
            // SAFETY: `pid` is the unreaped child returned by `fork`. Killing
            // the guard closes its probe pipe, so the descriptor-scrubbed
            // grandchild exits as well.
            unsafe {
                let _ = libc::kill(self.pid, libc::SIGKILL);
                loop {
                    if libc::waitpid(self.pid, std::ptr::null_mut(), 0) >= 0
                        || !last_errno_is_interrupted()
                    {
                        break;
                    }
                }
            }
        }

        fn arm(self) -> bool {
            write_byte(self.command_fd, GUARD_ARM) && self.read_ack() == Some(GUARD_ARM)
        }

        /// Returns `Some(true)` only after the guard sent SIGSTOP and this
        /// process subsequently resumed. `Some(false)` means a concurrent
        /// continue/termination cancelled the stop before it was issued.
        fn stop_parent(self) -> Option<bool> {
            if !write_byte(self.command_fd, GUARD_STOP) {
                return None;
            }
            match read_guard_ack_blocking(self.ack_fd)? {
                GUARD_STOPPED => Some(true),
                GUARD_CANCELLED => Some(false),
                _ => None,
            }
        }

        fn disarm(self) {
            let _ = write_byte(self.command_fd, GUARD_DISARM);
        }

        fn read_ack(self) -> Option<u8> {
            read_guard_ack(self.ack_fd, Duration::from_secs(2))
        }
    }

    fn read_guard_ack(fd: libc::c_int, timeout: Duration) -> Option<u8> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let timeout_ms = i32::try_from(remaining.as_millis().min(i32::MAX as u128))
                .unwrap_or(i32::MAX)
                .max(1);
            let mut poll_fd = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            // SAFETY: `poll_fd` is valid for one entry and the bounded timeout
            // prevents a failed guard wedging the signal monitor.
            let polled = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
            if polled == 0 {
                return None;
            }
            if polled < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return None;
            }
            let mut ack = 0_u8;
            // SAFETY: `fd` is the dedicated guard-to-parent pipe and `ack` is
            // valid writable storage for exactly one byte.
            let read = unsafe { libc::read(fd, (&mut ack as *mut u8).cast(), 1) };
            if read == 1 {
                return Some(ack);
            }
            if read == 0 {
                return None;
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return None;
            }
        }
    }

    /// Whether `expected` arrives on `fd` within `timeout`, skipping any other
    /// byte first. A reaper that came up after its READY deadline has that
    /// stale byte queued ahead of its CANCEL acknowledgement; judging the first
    /// byte alone failed a cancel the reaper had accepted (`C-004`).
    fn acknowledged(fd: libc::c_int, expected: u8, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match read_guard_ack(fd, remaining) {
                Some(byte) if byte == expected => return true,
                Some(_) => continue,
                None => return false,
            }
        }
    }

    fn read_guard_ack_blocking(fd: libc::c_int) -> Option<u8> {
        loop {
            let mut ack = 0_u8;
            // SAFETY: `fd` is the dedicated guard-to-parent pipe and `ack` is
            // valid writable storage for exactly one byte. This intentionally
            // blocks for the whole user-controlled suspension interval.
            let read = unsafe { libc::read(fd, (&mut ack as *mut u8).cast(), 1) };
            if read == 1 {
                return Some(ack);
            }
            if read == 0 {
                return None;
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return None;
            }
        }
    }

    fn write_byte(fd: libc::c_int, byte: u8) -> bool {
        loop {
            // SAFETY: `fd` is a dedicated pipe and `byte` remains valid for
            // the duration of the one-byte write.
            let written = unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
            if written == 1 {
                return true;
            }
            if written < 0 && last_errno_is_interrupted() {
                continue;
            }
            return false;
        }
    }

    fn spawn_guard(policy: SignalPolicy) -> Result<Guard, String> {
        // Resolve the descriptor ceiling before fork: sysconf may take libc
        // locks, whereas the multithreaded child may call only async-safe
        // primitives until it enters the guard loop.
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if open_max <= 0 {
            return Err("reading the Unix open-file descriptor ceiling".to_owned());
        }
        let open_max = libc::c_int::try_from(open_max)
            .map_err(|_| "Unix open-file descriptor ceiling exceeds c_int".to_owned())?;
        let command = create_cloexec_pipe()
            .map_err(|error| format!("creating Unix job-control command pipe: {error}"))?;
        let ack = match create_cloexec_pipe() {
            Ok(pipe) => pipe,
            Err(error) => {
                for fd in command {
                    close_fd(fd);
                }
                return Err(format!(
                    "creating Unix job-control acknowledgement pipe: {error}"
                ));
            }
        };
        let wake = match create_cloexec_pipe() {
            Ok(pipe) => pipe,
            Err(error) => {
                for fd in [command[0], command[1], ack[0], ack[1]] {
                    close_fd(fd);
                }
                return Err(format!("creating Unix job-control wake pipe: {error}"));
            }
        };
        let probe = match create_cloexec_pipe() {
            Ok(pipe) => pipe,
            Err(error) => {
                for fd in [command[0], command[1], ack[0], ack[1], wake[0], wake[1]] {
                    close_fd(fd);
                }
                return Err(format!("creating Unix job-control probe pipe: {error}"));
            }
        };
        if !set_nonblocking(wake[0]) || !set_nonblocking(wake[1]) {
            for fd in [
                command[0], command[1], ack[0], ack[1], wake[0], wake[1], probe[0], probe[1],
            ] {
                close_fd(fd);
            }
            return Err(format!(
                "creating Unix job-control wake/probe pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        GUARD_WAKE_FD.store(wake[1], Ordering::SeqCst);

        // SAFETY: the child enters `guard_loop` immediately, which uses only
        // libc syscalls and lock-free atomics after fork. It closes every
        // inherited descriptor except its two pipes before doing any work.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            GUARD_WAKE_FD.store(-1, Ordering::SeqCst);
            for fd in [
                command[0], command[1], ack[0], ack[1], wake[0], wake[1], probe[0], probe[1],
            ] {
                close_fd(fd);
            }
            return Err(format!(
                "starting Unix job-control guard: {}",
                std::io::Error::last_os_error()
            ));
        }
        if pid == 0 {
            // Replace inherited host callbacks and clear the inherited mask as
            // the first child-side action. Descriptor scrubbing can be long on
            // high-limit hosts; no signal in that window may run host code or
            // leave the only wake relay blocked.
            if !install_guard_dispositions(policy) {
                unsafe { libc::_exit(1) };
            }
            let parent = unsafe { libc::getppid() };
            let probe_pid = unsafe { libc::fork() };
            if probe_pid < 0 {
                unsafe { libc::_exit(1) };
            }
            if probe_pid == 0 {
                if !install_probe_dispositions() {
                    unsafe { libc::_exit(1) };
                }
                close_fd(probe[1]);
                close_inherited_fds(&[probe[0]], open_max);
                probe_loop(parent, probe[0]);
            }
            close_fd(command[1]);
            close_fd(ack[0]);
            close_fd(probe[0]);
            close_inherited_fds(&[command[0], ack[1], wake[0], wake[1], probe[1]], open_max);
            guard_loop(parent, command[0], ack[1], wake[0], probe[1], probe_pid);
        }

        GUARD_WAKE_FD.store(-1, Ordering::SeqCst);
        close_fd(ack[1]);
        close_fd(wake[0]);
        close_fd(wake[1]);
        close_fd(probe[0]);
        close_fd(probe[1]);
        if !set_nonblocking(command[1]) {
            for fd in [command[0], command[1], ack[0]] {
                close_fd(fd);
            }
            unsafe {
                let _ = libc::kill(pid, libc::SIGKILL);
                let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            return Err("configuring Unix job-control guard descriptors".to_owned());
        }
        let guard = Guard {
            command_fd: command[1],
            ack_fd: ack[0],
            _command_keepalive_fd: command[0],
            pid,
        };
        let mut probe_pid_bytes = [0_u8; 4];
        if guard.read_ack() != Some(GUARD_READY)
            || !read_raw_exact(ack[0], &mut probe_pid_bytes)
            || i32::from_ne_bytes(probe_pid_bytes) <= 0
        {
            for fd in [command[0], command[1], ack[0]] {
                close_fd(fd);
            }
            // SAFETY: `pid` is the child returned by fork and has not been
            // reaped. A failed setup acknowledgement must not leave it alive.
            unsafe {
                let _ = libc::kill(pid, libc::SIGKILL);
                let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            return Err("Unix job-control guard did not initialize".to_owned());
        }
        PROBE_PID.store(i32::from_ne_bytes(probe_pid_bytes), Ordering::SeqCst);
        GUARD_COMMAND_FD.store(command[1], Ordering::SeqCst);
        Ok(guard)
    }

    fn guard_loop(
        parent: libc::pid_t,
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        wake_fd: libc::c_int,
        probe_fd: libc::c_int,
        probe_pid: libc::pid_t,
    ) -> ! {
        let mut ready = [0_u8; 5];
        ready[0] = GUARD_READY;
        ready[1..].copy_from_slice(&probe_pid.to_ne_bytes());
        if !write_raw(ack_fd, &ready) {
            unsafe { libc::_exit(1) };
        }
        let mut armed = false;
        let mut stopping = false;
        let mut wake = false;
        let mut buffer = [0_u8; 64];
        loop {
            let mut poll_fds = [
                libc::pollfd {
                    fd: command_fd,
                    events: libc::POLLIN | libc::POLLHUP,
                    revents: 0,
                },
                libc::pollfd {
                    fd: wake_fd,
                    events: libc::POLLIN | libc::POLLHUP,
                    revents: 0,
                },
            ];
            // Both parent relays and guard-directed foreground signals make a
            // descriptor readable, so there is no atomic-check-to-poll window.
            // While the parent is SIGSTOPped, a signal sent only to its PID
            // cannot run a caught handler. Periodically resume only the parent;
            // its SA_SIGINFO SIGCONT handler recognizes this guard as sender,
            // delivers any pending Upstroke-owned termination, or immediately
            // re-stops. Agent groups remain stopped throughout.
            let timeout_ms = if armed && stopping { 250 } else { -1 };
            let polled = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, timeout_ms) };
            if polled < 0 && !last_errno_is_interrupted() {
                unsafe { libc::_exit(1) };
            }
            if polled == 0 && armed && stopping {
                if unsafe { libc::getppid() } != parent || !write_byte(probe_fd, GUARD_PROBE) {
                    unsafe { libc::_exit(0) };
                }
                continue;
            }
            if polled > 0 && poll_fds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                // SAFETY: `buffer` is valid writable storage and command_fd is
                // the guard's private read end.
                let count =
                    unsafe { libc::read(command_fd, buffer.as_mut_ptr().cast(), buffer.len()) };
                if count <= 0 {
                    unsafe { libc::_exit(0) };
                }
                for byte in &buffer[..count as usize] {
                    match *byte {
                        GUARD_ARM => {
                            // ARM is an epoch boundary. Signals observed before
                            // it are already represented in the parent's
                            // atomics and checked after this acknowledgement;
                            // retaining them would spuriously continue a later
                            // stop. A signal racing after this clear is caught
                            // by its ordered command or wake-pipe record.
                            wake = false;
                            stopping = false;
                            drain_pipe(wake_fd);
                            armed = true;
                            let _ =
                                unsafe { libc::write(ack_fd, (&GUARD_ARM as *const u8).cast(), 1) };
                        }
                        GUARD_STOP => {
                            if !armed || wake {
                                let _ = write_byte(ack_fd, GUARD_CANCELLED);
                                armed = false;
                                stopping = false;
                                wake = false;
                                continue;
                            }
                            // PID reuse must never redirect a late stop to an
                            // unrelated process. Reparenting proves the
                            // original Upstroke process is gone.
                            if unsafe { libc::getppid() } != parent {
                                unsafe { libc::_exit(0) };
                            }
                            // The parent is blocked reading this ack pipe. The
                            // stop is queued before the acknowledgement write,
                            // so it cannot return to userspace until a later
                            // SIGCONT has genuinely resumed it.
                            if unsafe { libc::kill(parent, libc::SIGSTOP) } != 0 {
                                unsafe { libc::_exit(0) };
                            }
                            stopping = true;
                        }
                        GUARD_DISARM => {
                            armed = false;
                            stopping = false;
                            wake = false;
                            drain_pipe(wake_fd);
                        }
                        _ => wake = true,
                    }
                }
            }
            if polled > 0 && poll_fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                drain_pipe(wake_fd);
                wake = true;
            }
            if armed && stopping && wake {
                // PID reuse must never redirect a late guard wake to an
                // unrelated process. Reparenting proves the original Upstroke
                // process is gone even if its numeric pid has been reused.
                if unsafe { libc::getppid() } != parent {
                    unsafe { libc::_exit(0) };
                }
                // SAFETY: a positive pid targets only the Upstroke parent. A
                // generated SIGCONT resumes it even while blocked or caught.
                if unsafe { libc::kill(parent, libc::SIGCONT) } != 0 {
                    unsafe { libc::_exit(0) };
                }
                if !write_byte(ack_fd, GUARD_STOPPED) {
                    unsafe { libc::_exit(0) };
                }
                armed = false;
                stopping = false;
                wake = false;
            }
        }
    }

    /// Remove every embedding-host callback from a fork-only helper.
    ///
    /// Signal numbers are sparse and platform-specific. `sigaction` reports
    /// EINVAL for holes, uncatchable signals, and values above the platform's
    /// range, so a fixed upper bound avoids non-portable NSIG APIs while still
    /// covering Linux real-time and BSD/macOS signals. Asynchronous signals
    /// are ignored so a broadcast cannot disable cleanup; synchronous faults
    /// retain their ordinary fatal behavior.
    fn scrub_private_helper_dispositions() -> bool {
        for signal in 1..=128 {
            if signal == libc::SIGKILL || signal == libc::SIGSTOP {
                continue;
            }
            let synchronous = matches!(
                signal,
                libc::SIGILL
                    | libc::SIGABRT
                    | libc::SIGFPE
                    | libc::SIGSEGV
                    | libc::SIGBUS
                    | libc::SIGTRAP
                    | libc::SIGSYS
            );
            let disposition = if synchronous {
                libc::SIG_DFL
            } else {
                libc::SIG_IGN
            };
            if !set_signal_disposition(signal, disposition) {
                return false;
            }
        }
        true
    }

    fn set_signal_disposition(signal: libc::c_int, disposition: libc::sighandler_t) -> bool {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = disposition;
            action.sa_flags = 0;
            if libc::sigemptyset(&mut action.sa_mask) != 0 {
                return false;
            }
            if libc::sigaction(signal, &action, std::ptr::null_mut()) == 0 {
                true
            } else {
                last_errno() == libc::EINVAL
            }
        }
    }

    fn install_probe_dispositions() -> bool {
        unsafe {
            if !scrub_private_helper_dispositions() {
                return false;
            }
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty) == 0
                && libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) == 0
        }
    }

    fn probe_loop(parent: libc::pid_t, command_fd: libc::c_int) -> ! {
        loop {
            let mut command = 0_u8;
            let read = unsafe { libc::read(command_fd, (&mut command as *mut u8).cast(), 1) };
            if read == 1 && command == GUARD_PROBE {
                if unsafe { libc::kill(parent, libc::SIGCONT) } == 0 {
                    continue;
                }
            } else if read < 0 && last_errno_is_interrupted() {
                continue;
            }
            unsafe { libc::_exit(0) };
        }
    }

    fn install_guard_dispositions(policy: SignalPolicy) -> bool {
        // The guard stays in the foreground process group but cannot join the
        // stop: it ignores SIGTSTP and records every transition that must wake
        // a parent already stopped by the guard. SIGSTOP itself targets only
        // the parent pid.
        unsafe {
            // Scrub before deliberately clearing the inherited mask. Only this
            // guard's narrow supervision surface is installed below.
            if !scrub_private_helper_dispositions() {
                return false;
            }
            // A library host may have blocked these signals on the thread
            // that first invoked Upstroke. The guard is an isolated relay, not
            // host code: clear its inherited mask so it can always wake a
            // parent that it previously stopped.
            let mut empty: libc::sigset_t = std::mem::zeroed();
            if libc::sigemptyset(&mut empty) != 0
                || libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) != 0
            {
                return false;
            }
            if policy.job_control {
                if libc::signal(libc::SIGTSTP, libc::SIG_IGN) == libc::SIG_ERR
                    || libc::signal(
                        libc::SIGCONT,
                        record_guard_signal as *const () as libc::sighandler_t,
                    ) == libc::SIG_ERR
                {
                    return false;
                }
            } else {
                // Job-control callbacks and defaults belong to the embedding
                // parent when Upstroke cannot safely proxy the pair. The private
                // guard must neither run fork-copied host code nor stop itself.
                if libc::signal(libc::SIGTSTP, libc::SIG_IGN) == libc::SIG_ERR
                    || libc::signal(libc::SIGCONT, libc::SIG_IGN) == libc::SIG_ERR
                {
                    return false;
                }
            }
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                if policy.wakes_guard(signal) {
                    // Custom callbacks belong to the embedding parent. Never
                    // run a fork-copied callback against the guard's private
                    // memory; translate it into the same self-pipe wake as a
                    // default Upstroke-owned termination signal instead.
                    if libc::signal(
                        signal,
                        record_guard_signal as *const () as libc::sighandler_t,
                    ) == libc::SIG_ERR
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn drain_pipe(fd: libc::c_int) {
        let mut buffer = [0_u8; 64];
        loop {
            // SAFETY: `buffer` is writable for its complete length and `fd` is
            // the nonblocking read side of the guard's private wake pipe.
            if unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) } <= 0 {
                return;
            }
        }
    }

    fn close_inherited_fds(keep: &[libc::c_int], open_max: libc::c_int) {
        // The fork must not retain the run lock, event file, pipes, or secrets.
        // Linux close_range keeps this bounded even when RLIMIT_NOFILE is in
        // the millions. Older kernels and other Unix hosts retain the
        // syscall-only per-descriptor fallback.
        #[cfg(target_os = "linux")]
        if close_ranges_except(keep) {
            return;
        }
        for fd in 0..open_max {
            if !keep.contains(&fd) {
                close_fd(fd);
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn close_ranges_except(keep: &[libc::c_int]) -> bool {
        let mut first = 0_u32;
        loop {
            let next_keep = keep
                .iter()
                .copied()
                .filter(|fd| *fd >= 0 && (*fd as u32) >= first)
                .map(|fd| fd as u32)
                .min();
            // `first == kept` is an empty range. Saturating `kept - 1`
            // would turn the fd-zero case into 0..=0 and close the descriptor
            // we were explicitly asked to preserve.
            if next_keep != Some(first) {
                let last = next_keep.map_or(u32::MAX, |fd| fd - 1);
                let result = unsafe { libc::syscall(libc::SYS_close_range, first, last, 0_u32) };
                if result != 0 {
                    return false;
                }
            }
            let Some(kept) = next_keep else {
                return true;
            };
            first = kept + 1;
        }
    }

    fn last_errno_is_interrupted() -> bool {
        last_errno() == libc::EINTR
    }

    fn last_errno() -> libc::c_int {
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location()
        }
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        }
    }

    fn raw_sleep_10ms() {
        let request = libc::timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        };
        let mut remaining = request;
        loop {
            let result = unsafe { libc::nanosleep(&remaining, &mut remaining) };
            if result == 0 || !last_errno_is_interrupted() {
                return;
            }
        }
    }

    fn verify_group_scanner() -> Result<(), String> {
        let own_group = unsafe { libc::getpgrp() };
        let own_pid = unsafe { libc::getpid() };
        // Process enumeration can race an unrelated process exiting. Retry a
        // bounded realistic interval, but refuse before launching an agent
        // when either cleanup enumeration or parent-state observation is
        // persistently absent (for example a Linux container without a
        // mounted/readable procfs).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match (
                group_has_non_zombie_members(own_group),
                process_is_stopped(own_pid),
            ) {
                (Some(true), Some(false)) => return Ok(()),
                (Some(false), _) => {
                    return Err(format!(
                        "Unix process-group scanner did not find the current group {own_group}"
                    ));
                }
                (Some(true), Some(true)) => {
                    return Err(
                        "Unix parent-state scanner reported the running Upstroke process as stopped"
                            .to_owned(),
                    );
                }
                _ if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(1));
                }
                _ => break,
            }
        }
        Err("Unix process-group scanner is unavailable; refusing to launch an agent whose cleanup could not be verified".to_owned())
    }

    #[cfg(target_os = "linux")]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum LinuxStatSnapshot {
        Present { pgid: i32, state: u8 },
        Vanished,
        Invalid,
    }

    #[cfg(target_os = "linux")]
    fn group_has_non_zombie_members(pgid: i32) -> Option<bool> {
        let directory = unsafe {
            libc::open(
                c"/proc".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if directory < 0 {
            return None;
        }
        let mut entries = [0_u8; 16_384];
        loop {
            let count = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    directory,
                    entries.as_mut_ptr(),
                    entries.len(),
                )
            };
            if count == 0 {
                close_fd(directory);
                return Some(false);
            }
            if count < 0 {
                if last_errno_is_interrupted() {
                    continue;
                }
                close_fd(directory);
                return None;
            }
            let mut offset = 0_usize;
            while offset < count as usize {
                if offset + 19 > count as usize {
                    close_fd(directory);
                    return None;
                }
                let record_len =
                    u16::from_ne_bytes([entries[offset + 16], entries[offset + 17]]) as usize;
                if record_len < 20 || offset + record_len > count as usize {
                    close_fd(directory);
                    return None;
                }
                let name = &entries[offset + 19..offset + record_len];
                let name_len = name
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(name.len());
                if let Some(pid) = parse_decimal(&name[..name_len]) {
                    match read_linux_stat_raw(pid) {
                        LinuxStatSnapshot::Present {
                            pgid: candidate,
                            state,
                        } if candidate == pgid && !matches!(state, b'Z' | b'X' | b'x') => {
                            close_fd(directory);
                            return Some(true);
                        }
                        LinuxStatSnapshot::Present { .. } | LinuxStatSnapshot::Vanished => {}
                        // Permission failures and malformed snapshots remain
                        // fail-closed. Only a kernel-confirmed vanished PID is
                        // safe to skip as ordinary process churn.
                        LinuxStatSnapshot::Invalid => {
                            close_fd(directory);
                            return None;
                        }
                    }
                }
                offset += record_len;
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn read_linux_stat_raw(pid: i32) -> LinuxStatSnapshot {
        let mut path = [0_u8; 64];
        let prefix = b"/proc/";
        path[..prefix.len()].copy_from_slice(prefix);
        let mut end = prefix.len();
        let Some(written) = write_decimal(pid, &mut path[end..]) else {
            return LinuxStatSnapshot::Invalid;
        };
        end += written;
        let suffix = b"/stat\0";
        let Some(target) = path.get_mut(end..end + suffix.len()) else {
            return LinuxStatSnapshot::Invalid;
        };
        target.copy_from_slice(suffix);
        let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return if matches!(last_errno(), libc::ENOENT | libc::ESRCH) {
                LinuxStatSnapshot::Vanished
            } else {
                LinuxStatSnapshot::Invalid
            };
        }
        let mut stat = [0_u8; 2_048];
        let count = loop {
            let read = unsafe { libc::read(fd, stat.as_mut_ptr().cast(), stat.len()) };
            if read < 0 && last_errno_is_interrupted() {
                continue;
            }
            break read;
        };
        let read_errno = (count < 0).then(last_errno);
        close_fd(fd);
        if matches!(read_errno, Some(libc::ENOENT | libc::ESRCH)) {
            return LinuxStatSnapshot::Vanished;
        }
        if count <= 0 {
            return LinuxStatSnapshot::Invalid;
        }
        parse_linux_stat_bytes(&stat[..count as usize])
            .map(|(pgid, state)| LinuxStatSnapshot::Present { pgid, state })
            .unwrap_or(LinuxStatSnapshot::Invalid)
    }

    #[cfg(target_os = "linux")]
    fn process_is_stopped(pid: i32) -> Option<bool> {
        match read_linux_stat_raw(pid) {
            LinuxStatSnapshot::Present { state, .. } => Some(matches!(state, b'T' | b't')),
            LinuxStatSnapshot::Vanished | LinuxStatSnapshot::Invalid => None,
        }
    }

    #[cfg(target_os = "linux")]
    fn parse_linux_stat_bytes(stat: &[u8]) -> Option<(i32, u8)> {
        let close = stat.iter().rposition(|byte| *byte == b')')?;
        let mut fields = stat.get(close + 1..)?;
        fields = trim_ascii_start(fields);
        let state = *fields.first()?;
        fields = next_ascii_field(fields)?.1;
        let (parent, tail) = next_ascii_field(fields)?;
        parse_decimal(parent)?;
        let (group, _) = next_ascii_field(tail)?;
        Some((parse_decimal(group)?, state))
    }

    #[cfg(target_os = "linux")]
    fn next_ascii_field(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
        let bytes = trim_ascii_start(bytes);
        let end = bytes
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(bytes.len());
        (end != 0).then_some((&bytes[..end], &bytes[end..]))
    }

    #[cfg(target_os = "linux")]
    fn trim_ascii_start(mut bytes: &[u8]) -> &[u8] {
        while bytes.first().is_some_and(u8::is_ascii_whitespace) {
            bytes = &bytes[1..];
        }
        bytes
    }

    #[cfg(target_os = "linux")]
    fn parse_decimal(bytes: &[u8]) -> Option<i32> {
        if bytes.is_empty() {
            return None;
        }
        let mut value = 0_i32;
        for byte in bytes {
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value.checked_mul(10)?.checked_add(i32::from(byte - b'0'))?;
        }
        Some(value)
    }

    #[cfg(target_os = "linux")]
    fn write_decimal(value: i32, output: &mut [u8]) -> Option<usize> {
        if value <= 0 {
            return None;
        }
        let mut reversed = [0_u8; 10];
        let mut count = 0_usize;
        let mut value = value as u32;
        while value != 0 {
            reversed[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
        }
        if output.len() < count {
            return None;
        }
        for index in 0..count {
            output[index] = reversed[count - index - 1];
        }
        Some(count)
    }

    #[cfg(target_os = "macos")]
    fn group_has_non_zombie_members(pgid: i32) -> Option<bool> {
        const PROC_PGRP_ONLY: u32 = 2;
        const MAX_PIDS: usize = 16_384;
        let mut pids = [0_i32; MAX_PIDS];
        let bytes = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                pgid as u32,
                pids.as_mut_ptr().cast(),
                std::mem::size_of_val(&pids) as libc::c_int,
            )
        };
        if bytes < 0 || bytes as usize == std::mem::size_of_val(&pids) {
            return None;
        }
        let count = bytes as usize / std::mem::size_of::<libc::pid_t>();
        for pid in &pids[..count] {
            if *pid <= 0 {
                continue;
            }
            let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
            let read = unsafe {
                libc::proc_pidinfo(
                    *pid,
                    libc::PROC_PIDT_SHORTBSDINFO,
                    // Apple only searches the zombie table for BSD-info
                    // flavors when this argument is non-zero. Without it an
                    // exited group member is indistinguishable from an
                    // incomplete snapshot and cleanup must wait forever.
                    1,
                    (&mut info as *mut libc::proc_bsdshortinfo).cast(),
                    std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int,
                )
            };
            if read != std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int {
                // A disappearing target-group pid is resolved by the next
                // complete snapshot. Never turn an incomplete observation
                // into permission to release the cleanup lease.
                return None;
            }
            if info.pbsi_pgid == pgid as u32 && info.pbsi_status != libc::SZOMB {
                return Some(true);
            }
        }
        Some(false)
    }

    #[cfg(target_os = "macos")]
    fn process_is_stopped(pid: i32) -> Option<bool> {
        let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
        let read = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDT_SHORTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdshortinfo).cast(),
                std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int,
            )
        };
        (read == std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int)
            .then_some(info.pbsi_status == libc::SSTOP)
    }

    #[cfg(target_os = "linux")]
    fn create_cloexec_pipe() -> Result<[libc::c_int; 2], std::io::Error> {
        let mut pipe = [-1; 2];
        // SAFETY: `pipe` exposes storage for exactly two descriptors. `pipe2`
        // applies CLOEXEC in the same kernel operation that publishes them, so
        // a concurrent spawn can never inherit an intermediate descriptor.
        if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) } == 0 {
            Ok(pipe)
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    #[cfg(target_os = "macos")]
    fn create_cloexec_pipe() -> Result<[libc::c_int; 2], std::io::Error> {
        use std::os::unix::ffi::OsStrExt;

        // Darwin has no pipe2. Build the anonymous-equivalent channel from a
        // FIFO inside an atomic, private mkdtemp directory: each endpoint is
        // opened with O_CLOEXEC in the syscall that creates its descriptor,
        // then the name and directory are removed before this function returns.
        let template = std::env::temp_dir().join(format!(".upstroke-pipe-{}-XXXXXX", unsafe {
            libc::getpid()
        }));
        let mut template = std::ffi::CString::new(template.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?
            .into_bytes_with_nul();
        if unsafe { libc::mkdtemp(template.as_mut_ptr().cast()) }.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        let directory_len = template.len().saturating_sub(1);
        let mut fifo = Vec::with_capacity(directory_len + b"/channel\0".len());
        fifo.extend_from_slice(&template[..directory_len]);
        fifo.extend_from_slice(b"/channel\0");
        let cleanup = || unsafe {
            let _ = libc::unlink(fifo.as_ptr().cast());
            let _ = libc::rmdir(template.as_ptr().cast());
        };
        if unsafe { libc::mkfifo(fifo.as_ptr().cast(), 0o600) } != 0 {
            let error = std::io::Error::last_os_error();
            cleanup();
            return Err(error);
        }

        let read_fd = unsafe {
            libc::open(
                fifo.as_ptr().cast(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if read_fd < 0 {
            let error = std::io::Error::last_os_error();
            cleanup();
            return Err(error);
        }
        let write_fd = unsafe {
            libc::open(
                fifo.as_ptr().cast(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if write_fd < 0 {
            let error = std::io::Error::last_os_error();
            close_fd(read_fd);
            cleanup();
            return Err(error);
        }
        let unlinked = unsafe { libc::unlink(fifo.as_ptr().cast()) } == 0;
        let removed = unsafe { libc::rmdir(template.as_ptr().cast()) } == 0;
        if !unlinked || !removed || !clear_nonblocking(read_fd) || !clear_nonblocking(write_fd) {
            let error = std::io::Error::last_os_error();
            close_fd(read_fd);
            close_fd(write_fd);
            return Err(error);
        }
        Ok([read_fd, write_fd])
    }

    #[cfg(target_os = "macos")]
    fn clear_nonblocking(fd: libc::c_int) -> bool {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) == 0
        }
    }

    fn set_nonblocking(fd: libc::c_int) -> bool {
        // Signal handlers may write this descriptor. Nonblocking mode makes a
        // dead or unresponsive guard fail closed instead of wedging Upstroke in
        // async-signal context.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                return libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0;
            }
        }
        false
    }

    fn close_fd(fd: libc::c_int) {
        if fd >= 0 {
            // SAFETY: callers transfer ownership of each raw descriptor here.
            let _ = unsafe { libc::close(fd) };
        }
    }

    fn signal_groups(groups: &[i32], signal: libc::c_int) {
        for pgid in groups {
            // SAFETY: every registered child was created with
            // `process_group(0)`, so its pid is its private group id. A
            // negative id targets that group and never Upstroke's group.
            let _ = unsafe { libc::kill(-*pgid, signal) };
        }
    }

    fn stop_groups(groups: &[i32]) -> bool {
        signal_groups(groups, libc::SIGSTOP);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if groups_are_quiescent(groups) {
                return true;
            }
            if std::time::Instant::now() >= deadline
                || PENDING_TERMINATION.load(Ordering::SeqCst) != 0
                || CONTINUE_REQUESTED.load(Ordering::SeqCst)
            {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[cfg(target_os = "linux")]
    fn groups_are_quiescent(groups: &[i32]) -> bool {
        if groups.is_empty() {
            return true;
        }
        // `/proc/<pid>/stat` is a kernel interface and remains available on
        // distributions such as NixOS that intentionally have no `/bin/ps`.
        // It observes every descendant in the process group, not only the
        // direct child that Upstroke can wait on.
        let entries = match std::fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(_) => return false,
        };
        let mut observed = vec![false; groups.len()];
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            if entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
                .is_none()
            {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                // Processes can disappear between directory enumeration and
                // the read. A still-live target group is caught by either
                // another member or the kill(0) completeness check below.
                continue;
            };
            let Some((pgid, state)) = parse_linux_process_stat(&stat) else {
                return false;
            };
            let Some(index) = groups.iter().position(|candidate| *candidate == pgid) else {
                continue;
            };
            observed[index] = true;
            if !matches!(state, b'T' | b't' | b'Z' | b'X' | b'x') {
                return false;
            }
        }
        quiescent_snapshot_is_complete(groups, &observed)
    }

    #[cfg(target_os = "linux")]
    fn parse_linux_process_stat(stat: &str) -> Option<(i32, u8)> {
        // The parenthesized command may itself contain spaces and `)` bytes;
        // the final close parenthesis is the only reliable field boundary.
        let tail = stat.get(stat.rfind(')')? + 1..)?.trim_start();
        let mut fields = tail.split_whitespace();
        let state = *fields.next()?.as_bytes().first()?;
        let _parent_pid = fields.next()?.parse::<i32>().ok()?;
        let process_group = fields.next()?.parse::<i32>().ok()?;
        Some((process_group, state))
    }

    #[cfg(not(target_os = "linux"))]
    fn groups_are_quiescent(groups: &[i32]) -> bool {
        if groups.is_empty() {
            return true;
        }
        // `/bin/ps` is a fixed base-system interface on macOS; no
        // repository-controlled PATH entry can substitute for it.
        let output = match std::process::Command::new("/bin/ps")
            .args(["-axo", "pgid=,stat="])
            .env("LC_ALL", "C")
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => return false,
        };
        let listing = String::from_utf8_lossy(&output.stdout);
        let mut observed = vec![false; groups.len()];
        for line in listing.lines() {
            let mut fields = line.split_whitespace();
            let Some(pgid) = fields.next().and_then(|field| field.parse::<i32>().ok()) else {
                continue;
            };
            let Some(index) = groups.iter().position(|candidate| *candidate == pgid) else {
                continue;
            };
            observed[index] = true;
            let Some(state) = fields.next().and_then(|field| field.as_bytes().first()) else {
                return false;
            };
            if !matches!(*state, b'T' | b'Z' | b'X') {
                return false;
            }
        }
        quiescent_snapshot_is_complete(groups, &observed)
    }

    fn quiescent_snapshot_is_complete(groups: &[i32], observed: &[bool]) -> bool {
        for (index, pgid) in groups.iter().enumerate() {
            if observed[index] {
                continue;
            }
            // A group that disappeared between SIGSTOP and the snapshot is
            // already quiescent. Any other result means `ps` failed to account
            // for a still-live member, so do not stop the parent yet.
            if unsafe { libc::kill(-*pgid, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
            {
                return false;
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // The container half of the orphan window — ST-16 (d)
    // -----------------------------------------------------------------------

    /// The `docker` argument vectors, rendered before any fork.
    ///
    /// A reaper is a `fork`-only child of a multithreaded process: after the
    /// fork it may call only async-signal-safe functions, so it can neither
    /// format a filter nor allocate an argv. Every byte it will ever need is
    /// therefore built here, on the parent side, exactly as `spawn_reaper`'s
    /// `cleanup_paths` are — and a `CString`'s buffer does not move when the
    /// struct that owns it does, so the pointer array stays valid.
    struct ReaperContainers {
        program: std::ffi::CString,
        /// Kept alive for the pointers in `ps_argv`.
        _ps: Vec<std::ffi::CString>,
        /// NULL-terminated `argv` for `docker ps …`.
        ps_argv: Vec<*const libc::c_char>,
    }

    /// The scope every reaper started from now on inherits, or `None`.
    ///
    /// A reaper already running keeps the scope it was forked with; there is no
    /// channel for handing one a new one, and inventing a wire frame for it
    /// would put a variable-length message into a protocol whose frames are five
    /// bytes.
    static CONTAINER_SCOPE: OnceLock<
        Mutex<Option<crate::runner::container::census::ReaperContainerScope>>,
    > = OnceLock::new();

    /// The fixed listing buffer. A `--no-trunc` id is 64 bytes plus a newline,
    /// so this is **126 containers per listing** — and a reaper cannot grow a
    /// buffer, so the number of *rounds* is what has to be unbounded. It was
    /// `8`, which made the buffer size a silent ceiling of 126 x 8 = **1,008
    /// containers**: a coordinator dying with 1,009 of them left one behind and
    /// reported the same success it reports on a clean machine.
    const REAPER_PS_BUFFER: usize = 8192;

    /// The ceiling on one `docker` invocation, in 10 ms ticks.
    ///
    /// `determinism` forbids sleeps in tests and this is not one: it is the
    /// fail-safe that keeps a wedged daemon from holding R28 — the shared
    /// cleanup hold the next coordinator waits on — for ever. A reaper that
    /// waited without a bound would convert "docker is hung" into "no run on
    /// this machine can ever start again".
    const REAPER_DOCKER_TICKS: usize = 3_000;

    /// Arm or disarm the container scope. See
    /// [`super::set_container_reclaim_scope`].
    pub(super) fn set_container_reclaim_scope(
        scope: Option<&crate::runner::container::census::ReaperContainerScope>,
    ) -> Result<(), UpstrokeError> {
        // Rendered here so a scope that cannot be turned into argv is refused
        // by the caller that set it, rather than silently doing nothing inside
        // a reaper that has no error channel.
        if let Some(scope) = scope {
            render_container_argv(scope)?;
        }
        let mut held = CONTAINER_SCOPE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = scope.cloned();
        Ok(())
    }

    /// The absolute program the reaper will `execv`, resolved **before** the
    /// fork.
    ///
    /// **`execv` does not search `PATH`. Only `execvp` does** — and `execvp` is
    /// not on the POSIX async-signal-safe list, so a reaper (a `fork`-only child
    /// of a multithreaded process) may not call it. A bare `docker` handed to
    /// `execv` therefore resolves against nothing at all: the listing child
    /// `_exit(127)`s, the pipe carries no bytes, and the reaper reports exactly
    /// the same success it reports on a clean machine. Measured, not reasoned —
    /// it is what shipped, and the only fixture used an absolute stub.
    ///
    /// So the search happens here, on the parent side, in ordinary code with an
    /// error channel: the same discipline that renders every other byte the
    /// reaper needs before the fork. `execvp`'s own rule is mirrored exactly —
    /// a name containing a `/` is a path and is used verbatim (that is what
    /// `execv` already does correctly); a name with no `/` is searched for on
    /// `PATH`, and one `PATH` cannot resolve is **refused** rather than handed
    /// to a child that has no way to say so.
    ///
    /// [`crate::util::find_program`] is the resolver deliberately: it is the
    /// one `runner::container::DockerCli::available` asks when it decides the
    /// runtime is present, so the reaper execs the binary the rest of the engine
    /// means by `docker`.
    fn resolve_reaper_program(program: &std::path::Path) -> Result<PathBuf, UpstrokeError> {
        use std::os::unix::ffi::OsStrExt as _;
        if program.as_os_str().as_bytes().contains(&b'/') {
            return Ok(program.to_path_buf());
        }
        let refused = |why: &str| UpstrokeError::Refused {
            message: format!(
                "the Unix reaper's container scope names the program `{}`, which {why}; a reaper \
                 is a fork-only child restricted to async-signal-safe calls, so it must be handed \
                 an already-resolved path — `execv` does not search `PATH` and `execvp` is not \
                 async-signal-safe — and a scope whose program cannot be resolved is refused here \
                 rather than silently reclaiming nothing",
                program.display()
            ),
        };
        let name = program.to_str().ok_or_else(|| {
            refused("carries no separator and is not UTF-8, so it cannot be looked up on `PATH`")
        })?;
        crate::util::find_program(name)
            .ok_or_else(|| refused("carries no separator and is not on this process's `PATH`"))
    }

    /// The argument vectors for `scope`, or why they cannot be built.
    fn render_container_argv(
        scope: &crate::runner::container::census::ReaperContainerScope,
    ) -> Result<ReaperContainers, UpstrokeError> {
        let nul = |value: &str| UpstrokeError::Refused {
            message: format!(
                "the Unix reaper's container scope renders `{value}`, which carries an interior \
                 NUL and cannot be an argument to `{}`",
                scope.program().display()
            ),
        };
        let resolved = resolve_reaper_program(scope.program())?;
        let program = std::ffi::CString::new(resolved.as_os_str().as_encoded_bytes())
            .map_err(|_| nul(&resolved.to_string_lossy()))?;
        let mut ps = Vec::new();
        for (index, argument) in scope.list_argv().into_iter().enumerate() {
            // `argv[0]` is the resolved path too, so the program the child execs
            // and the program it reports itself as are one string in every one
            // of the three invocations — `ps` here, `kill` and `rm` from
            // `containers.program` directly.
            let argument = if index == 0 {
                resolved.to_string_lossy().into_owned()
            } else {
                argument
            };
            ps.push(std::ffi::CString::new(argument.clone()).map_err(|_| nul(&argument))?);
        }
        let mut ps_argv: Vec<*const libc::c_char> =
            ps.iter().map(|argument| argument.as_ptr()).collect();
        ps_argv.push(std::ptr::null());
        Ok(ReaperContainers {
            program,
            _ps: ps,
            ps_argv,
        })
    }

    /// What a reaper about to be forked should carry.
    fn container_scope_for_a_new_reaper() -> Option<ReaperContainers> {
        let scope = CONTAINER_SCOPE
            .get()?
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()?;
        render_container_argv(&scope).ok()
    }

    /// Kill and remove every labeled container of the dead coordinator.
    ///
    /// `T-CONTAINER.resume_action`: "on Unix the cleanup reaper performs
    /// **kill/rm** earlier when the coordinator dies". Only kill and rm: the
    /// Git view and the intent record are removed by the next write command's
    /// census, which is why every step of `runner::container::reclaim` is
    /// idempotent and tolerant of already-gone.
    ///
    /// Every call here is async-signal-safe: `fork`, `execv`, `pipe`, `dup2`,
    /// `open`, `close`, `poll`, `read`, `waitpid`, `kill`, `_exit`.
    fn reclaim_labeled_containers(containers: &ReaperContainers) {
        // Two fixed buffers and no round counter. The loop ends on one of three
        // conditions, and none of them is a container count:
        //
        // 1. the listing is **empty** — everything this selector names is gone;
        // 2. the listing is **byte-identical to the previous round's** — the
        //    runtime answered with exactly what it answered before, so the
        //    `kill`/`rm` of that round removed nothing and another round would
        //    repeat it. This is the real form of the guard the round count was
        //    standing in for ("a runtime that keeps reporting the same
        //    container cannot hold R28 for ever"), and it is both tighter (it
        //    fires on the second round rather than the eighth) and not a
        //    ceiling on how much work a healthy runtime may be given;
        // 3. no complete id was parsed out of a non-empty listing.
        //
        // Termination without a count: the selector names one **dead**
        // incarnation, so nothing can add to the set while this runs — the only
        // process that creates containers under that incarnation label is the
        // coordinator that died. The set is therefore finite and non-growing,
        // each round either shrinks it or answers identically, and every
        // `docker` invocation inside a round is itself bounded by
        // [`REAPER_DOCKER_TICKS`].
        //
        // Stopping on (2) is not a silent give-up: what it leaves behind is a
        // labeled container the runtime will not remove, and that is exactly
        // the residue the **next write command's census** is required to refuse
        // over — `refusal_condition`'s "a dead owner's or dead incarnation's
        // labeled container that cannot be observed terminated blocks
        // admission". The reaper closes the window early; the census is what
        // makes failing to close it loud.
        let mut buffer = [0_u8; REAPER_PS_BUFFER];
        let mut previous = [0_u8; REAPER_PS_BUFFER];
        let mut previous_filled = usize::MAX;
        loop {
            let filled = list_labeled_containers(containers, &mut buffer);
            if filled == 0 {
                return;
            }
            if filled == previous_filled && buffer[..filled] == previous[..filled] {
                return;
            }
            previous[..filled].copy_from_slice(&buffer[..filled]);
            previous_filled = filled;
            let mut settled = 0_usize;
            let mut start = 0_usize;
            for index in 0..filled {
                if buffer[index] != b'\n' {
                    continue;
                }
                // NUL-terminate the id where it lies. Nothing is allocated and
                // nothing is copied; the buffer is this frame's own.
                buffer[index] = 0;
                if index > start {
                    let id = buffer[start..].as_ptr().cast::<libc::c_char>();
                    let kill: [*const libc::c_char; 4] = [
                        containers.program.as_ptr(),
                        c"kill".as_ptr(),
                        id,
                        std::ptr::null(),
                    ];
                    spawn_docker(containers.program.as_ptr(), kill.as_ptr());
                    // `--volumes`, exactly as `DockerCli::remove` issues it
                    // (`PR6-ACCT-006`). An image declaring `VOLUME` gets one
                    // **anonymous** volume per container, and `docker rm`
                    // without this leaves one behind for every container the
                    // reaper removes: measured, 29 leaked from a single run of
                    // this suite through the ordinary path before
                    // `PR6A-ANONYMOUS-VOLUMES-LEAK` put the flag there. Those
                    // volumes are R26 — created by `docker create` as part of
                    // the container, referable by nothing else — and once the
                    // reaper has removed the container the following
                    // intent-only census has no handle on them at all, so this
                    // is the *only* point at which they can be reclaimed.
                    // `--volumes` removes anonymous volumes and **never a named
                    // one**, so it cannot touch R20 (measured on docker 29.7.2:
                    // a mounted named volume survives `rm --force --volumes`).
                    let remove: [*const libc::c_char; 6] = [
                        containers.program.as_ptr(),
                        c"rm".as_ptr(),
                        c"--force".as_ptr(),
                        c"--volumes".as_ptr(),
                        id,
                        std::ptr::null(),
                    ];
                    spawn_docker(containers.program.as_ptr(), remove.as_ptr());
                    settled = settled.saturating_add(1);
                }
                start = index + 1;
            }
            if settled == 0 {
                return;
            }
        }
    }

    /// Run `docker ps …` and read its ids into `buffer`, returning how many
    /// bytes arrived.
    fn list_labeled_containers(containers: &ReaperContainers, buffer: &mut [u8]) -> usize {
        let mut fds = [0 as libc::c_int; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return 0;
        }
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            close_fd(fds[0]);
            close_fd(fds[1]);
            return 0;
        }
        if pid == 0 {
            unsafe {
                // The reaper closed every inherited descriptor including 0, 1
                // and 2, so `pipe` may well have handed back fd 0 and fd 1
                // themselves. Move the write end onto stdout only when it is
                // not already there, and never close the descriptor that IS
                // stdout: doing so leaves `docker ps` writing to a closed fd,
                // the listing empty, and nothing reclaimed — with the reaper
                // reporting exactly the same success it reports on a clean
                // machine. Measured, not reasoned: it is what happened.
                if fds[1] != 1 && libc::dup2(fds[1], 1) < 0 {
                    libc::_exit(127);
                }
                if fds[0] != 1 {
                    close_fd(fds[0]);
                }
                if fds[1] != 1 {
                    close_fd(fds[1]);
                }
                quiet_standard_descriptors();
                libc::execv(containers.program.as_ptr(), containers.ps_argv.as_ptr());
                libc::_exit(127);
            }
        }
        close_fd(fds[1]);
        let filled = read_bounded(fds[0], buffer);
        close_fd(fds[0]);
        reap_bounded(pid);
        filled
    }

    /// `docker <verb> <id>`, output discarded, bounded.
    fn spawn_docker(program: *const libc::c_char, argv: *const *const libc::c_char) {
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return;
        }
        if pid == 0 {
            unsafe {
                quiet_standard_descriptors();
                libc::execv(program, argv);
                libc::_exit(127);
            }
        }
        reap_bounded(pid);
    }

    /// Give the exec'd `docker` real standard descriptors.
    ///
    /// The reaper closed every inherited descriptor including 0, 1 and 2, so
    /// without this a `docker` that opened a file would be handed **fd 1 or fd
    /// 2** for it and would then write its output or its diagnostics into that
    /// file. `/dev/null` on whichever of the three is still free is the
    /// cheapest way to make the numbers mean what they mean.
    ///
    /// A descriptor that is **already** open is left alone, which is what keeps
    /// this from undoing the listing child's pipe on fd 1.
    unsafe fn quiet_standard_descriptors() {
        unsafe {
            // In this order: `open` returns the lowest free descriptor, so
            // filling 0 first is what lets 1 and 2 land where they are asked
            // for without a `dup2` at all.
            ensure_standard_descriptor(0, libc::O_RDONLY);
            ensure_standard_descriptor(1, libc::O_WRONLY);
            ensure_standard_descriptor(2, libc::O_WRONLY);
        }
    }

    /// Open `/dev/null` onto `target` unless something is already there.
    unsafe fn ensure_standard_descriptor(target: libc::c_int, flags: libc::c_int) {
        unsafe {
            if libc::fcntl(target, libc::F_GETFD) != -1 {
                return;
            }
            let opened = libc::open(c"/dev/null".as_ptr(), flags);
            if opened < 0 {
                return;
            }
            if opened != target {
                let _ = libc::dup2(opened, target);
                close_fd(opened);
            }
        }
    }

    /// Read until EOF, the buffer is full, or the ceiling is reached.
    fn read_bounded(fd: libc::c_int, buffer: &mut [u8]) -> usize {
        let mut used = 0_usize;
        let mut ticks = 0_usize;
        while used < buffer.len() {
            let mut waiting = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut waiting, 1, 10) };
            if ready < 0 {
                if last_errno_is_interrupted() {
                    continue;
                }
                return used;
            }
            if ready == 0 {
                ticks = ticks.saturating_add(1);
                if ticks >= REAPER_DOCKER_TICKS {
                    return used;
                }
                continue;
            }
            let read = unsafe {
                libc::read(
                    fd,
                    buffer.as_mut_ptr().add(used).cast(),
                    buffer.len() - used,
                )
            };
            if read > 0 {
                used += read as usize;
            } else if read < 0 && last_errno_is_interrupted() {
                continue;
            } else {
                return used;
            }
        }
        used
    }

    /// Wait for one `docker`, and kill it rather than hold R28 for ever.
    fn reap_bounded(pid: libc::pid_t) {
        for _ in 0..REAPER_DOCKER_TICKS {
            let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
            if waited == pid {
                return;
            }
            if waited < 0 && !last_errno_is_interrupted() {
                return;
            }
            raw_sleep_10ms();
        }
        unsafe {
            let _ = libc::kill(pid, libc::SIGKILL);
        }
        loop {
            let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
            if waited == pid || (waited < 0 && !last_errno_is_interrupted()) {
                return;
            }
        }
    }

    /// What a reaper does when its **coordinator has died**: settle the group,
    /// then close the container half of the orphan window.
    ///
    /// Separate from the [`REAPER_CLEANUP`] path on purpose, and this is the
    /// distinction the whole extension turns on. `REAPER_CLEANUP` and
    /// [`REAPER_CANCEL`] are the **live** coordinator asking for its invocation
    /// to be settled; killing its labeled containers there would kill the
    /// containers of a coordinator that is still spending through them, which is
    /// `authoritative_state`'s "a live incarnation's containers must not be
    /// touched" — the opposite of what this exists for.
    fn settle_after_coordinator_death(
        pgid: i32,
        anchor: libc::pid_t,
        cleanup_delay_ms: u64,
        containers: Option<&ReaperContainers>,
    ) {
        if pgid > 0 {
            cleanup_reaper_group(pgid, anchor, cleanup_delay_ms);
        }
        if let Some(containers) = containers {
            reclaim_labeled_containers(containers);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        static REAPED_CHILD_STOP: AtomicBool = AtomicBool::new(false);

        /// R28 is a **shared** hold, and one run has more than one reaper.
        ///
        /// `resource_accounting.rows[R28].resource` — "a surviving Unix cleanup
        /// reaper's shared `cleanup.lock` hold (**one per reaper**; a reaper
        /// may outlive the coordinator while it settles its process groups)".
        /// Narrowing this `flock` to `LOCK_EX` would let the first reaper of a
        /// run take the hold and refuse every later one — the second concurrent
        /// invocation failing to start at all — and nothing observed it,
        /// because no test ran two overlapping invocations and inspected their
        /// holds.
        ///
        /// `flock` holds belong to the open file description, so two calls here
        /// are exactly two independent holders, which is what a second reaper
        /// is. The expected behaviour is `flock(2)`'s, not this function's:
        /// shared holds coexist and both exclude the exclusive side.
        #[test]
        fn the_reapers_cleanup_hold_is_shared_between_overlapping_invocations() {
            use std::os::unix::ffi::OsStrExt;

            let path = std::env::temp_dir().join(format!(
                "upstroke-r28-shared-{}-{}.lock",
                std::process::id(),
                crate::ulid::ulid()
            ));
            std::fs::write(&path, b"").expect("create a cleanup lease file");
            let target = std::ffi::CString::new(path.as_os_str().as_bytes())
                .expect("a temporary path without a null byte");
            let held = std::slice::from_ref(&target);

            assert!(
                lock_cleanup_paths(held),
                "the first invocation's reaper could not take R28 at all"
            );
            assert!(
                lock_cleanup_paths(held),
                "a second overlapping invocation's reaper was refused the shared hold: \
                 R28 is `one per reaper`, not one per run"
            );

            // And both holds still exclude the next coordinator, which is the
            // other half of R28: `observed (never owned or reset) by the next
            // coordinator … through the exclusive cleanup probe`.
            // SAFETY: a null-terminated path this test created; a failure
            // returns a negative descriptor.
            let fd = unsafe { libc::open(target.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            assert!(fd >= 0, "reopening the lease file");
            // SAFETY: `fd` is live and owned here until it is closed.
            let exclusive = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            let errno = last_errno();
            close_fd(fd);
            let _ = std::fs::remove_file(&path);
            assert_ne!(
                exclusive, 0,
                "the exclusive side was granted while two reapers held R28"
            );
            assert!(
                errno == libc::EWOULDBLOCK || errno == libc::EAGAIN,
                "the exclusive probe failed for an unrelated reason: {errno}"
            );
        }

        #[test]
        fn reaper_distinguishes_a_probe_pulse_from_a_stable_parent_resume() {
            let mut running_polls = 0;
            for _ in 1..REAPER_RESUME_STABLE_POLLS {
                assert!(!parent_has_stably_resumed(Some(false), &mut running_polls));
            }
            assert!(!parent_has_stably_resumed(Some(true), &mut running_polls));
            assert_eq!(running_polls, 0);

            for _ in 1..REAPER_RESUME_STABLE_POLLS {
                assert!(!parent_has_stably_resumed(Some(false), &mut running_polls));
            }
            assert!(parent_has_stably_resumed(Some(false), &mut running_polls));
            assert!(!parent_has_stably_resumed(None, &mut running_polls));
            assert_eq!(running_polls, 0);
        }

        extern "C" fn reap_child_transitions(_: libc::c_int) {
            if REAPED_CHILD_STOP.swap(true, Ordering::SeqCst) {
                return;
            }
            let mut status = 0;
            let child = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG | libc::WUNTRACED) };
            if child > 0 && libc::WIFSTOPPED(status) {
                // Keep a broken implementation from leaking the consumed,
                // permanently stopped anchor after this regression fails.
                let _ = unsafe { libc::kill(child, libc::SIGKILL) };
            }
        }

        #[test]
        #[ignore = "subprocess helper"]
        fn sigchld_reaper_host_helper() {
            if std::env::var_os("UPSTROKE_SIGCHLD_REAPER_HELPER").is_none() {
                return;
            }
            REAPED_CHILD_STOP.store(false, Ordering::SeqCst);
            assert_ne!(
                unsafe {
                    libc::signal(
                        libc::SIGCHLD,
                        reap_child_transitions as *const () as libc::sighandler_t,
                    )
                },
                libc::SIG_ERR
            );
            let target = unsafe { libc::fork() };
            if target == 0 {
                let _ = unsafe { libc::setpgid(0, 0) };
                loop {
                    unsafe { libc::pause() };
                }
            }
            assert!(target > 0);
            let result = unsafe { libc::setpgid(target, target) };
            assert!(
                result == 0 || matches!(last_errno(), libc::EACCES | libc::EPERM),
                "setpgid: {}",
                std::io::Error::last_os_error()
            );

            let reaper = spawn_reaper().expect("spawn private reaper");
            assert!(reaper.register_raw(target), "register target group");
            assert!(reaper.cleanup(target), "cleanup target group");
            let _ = unsafe { libc::waitpid(target, std::ptr::null_mut(), 0) };
        }

        #[test]
        fn a_host_sigchld_reaper_cannot_consume_the_private_anchor() {
            use std::os::unix::process::CommandExt;

            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args(["sigchld_reaper_host_helper", "--ignored", "--nocapture"])
                .env("UPSTROKE_SIGCHLD_REAPER_HELPER", "1")
                .process_group(0)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = command.spawn().expect("spawn SIGCHLD helper");
            let pid = i32::try_from(child.id()).expect("helper pid");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = child.try_wait().expect("poll SIGCHLD helper") {
                    assert!(status.success(), "SIGCHLD helper status: {status}");
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
                    let _ = child.wait();
                    panic!("inherited SIGCHLD reaper consumed the private anchor transition");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }

        /// Subprocess entry for the two `C-004` handshake checks.
        ///
        /// In a fresh process because a regression of either arms process-wide
        /// `SIGTERM`, which must fail this one test rather than the harness
        /// running it — the exact shape `C-004` was.
        #[test]
        #[ignore = "subprocess helper"]
        fn reaper_handshake_helper() {
            if std::env::var_os("UPSTROKE_REAPER_HANDSHAKE_HELPER").is_none() {
                return;
            }
            // The parent holds READY back past the 2 s deadline, and past the
            // further 2 s a cancel would then have waited, through
            // `UPSTROKE_TEST_REAPER_READY_DELAY_MS`. The launch must fail
            // promptly, arm nothing, and leave no child behind.
            let started = Instant::now();
            let launch = spawn_reaper();
            let elapsed = started.elapsed();
            assert!(
                launch.is_err(),
                "a reaper that missed its READY deadline was accepted as initialized"
            );
            assert!(
                elapsed < Duration::from_secs(4),
                "the late reaper held the launch for {elapsed:?}, past its own deadline"
            );
            assert_eq!(
                PENDING_TERMINATION.load(Ordering::SeqCst),
                0,
                "a reaper that missed its READY deadline armed process-wide termination"
            );
            // SAFETY: `waitpid(-1, WNOHANG)` inspects only this process's
            // children, and the late reaper is the only child it has had.
            let waited = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
            assert!(
                waited < 0 && !last_errno_is_interrupted(),
                "the late reaper was left behind (waitpid(-1, WNOHANG) returned {waited})"
            );

            // A CANCEL acknowledged behind a stale READY is a cancel that
            // succeeded: a reaper that came up late queues READY ahead of its
            // OK, and judging the first byte alone failed it.
            let command = create_cloexec_pipe().expect("a stand-in command pipe");
            let ack = create_cloexec_pipe().expect("a stand-in acknowledgement pipe");
            // SAFETY: the forked child calls only `_exit`.
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                unsafe { libc::_exit(0) };
            }
            assert!(pid > 0, "fork a stand-in reaper for the cancel to reap");
            assert!(write_raw(ack[1], &[REAPER_READY]), "queue the stale READY");
            let answer = thread::spawn(move || {
                let mut frame = [0_u8; 5];
                read_raw_exact(command[0], &mut frame)
                    && frame[0] == REAPER_CANCEL
                    && write_raw(ack[1], &[REAPER_OK])
            });
            Reaper {
                command_fd: command[1],
                ack_fd: ack[0],
                _command_keepalive_fd: command[0],
                pid,
            }
            .cancel();
            assert!(
                answer.join().expect("the stand-in reaper thread"),
                "the stand-in reaper never saw CANCEL"
            );
            close_fd(ack[1]);
            assert_eq!(
                PENDING_TERMINATION.load(Ordering::SeqCst),
                0,
                "a CANCEL acknowledged behind a stale READY armed process-wide termination"
            );
        }

        /// `C-004`: a reaper that misses its READY deadline is an ordinary
        /// failed launch, and a CANCEL acknowledged behind a stale READY is a
        /// cancel that succeeded. Neither arms process-wide termination.
        #[test]
        fn a_late_reaper_fails_its_launch_without_arming_termination() {
            use std::os::unix::process::CommandExt;

            let output = Command::new(std::env::current_exe().expect("test executable"))
                .args(["reaper_handshake_helper", "--ignored", "--nocapture"])
                .env("UPSTROKE_REAPER_HANDSHAKE_HELPER", "1")
                .env("UPSTROKE_TEST_REAPER_READY_DELAY_MS", "4500")
                .process_group(0)
                .stdin(Stdio::null())
                .output()
                .expect("run the reaper handshake helper");
            assert!(
                output.status.success(),
                "reaper handshake helper: {}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        #[cfg(target_os = "linux")]
        #[test]
        #[ignore = "subprocess helper"]
        fn linux_close_range_fd_zero_helper() {
            if std::env::var_os("UPSTROKE_CLOSE_RANGE_FD_ZERO_HELPER").is_none() {
                return;
            }

            // The Rust test harness may reopen a missing standard descriptor
            // during startup, so close it at the final isolated point before
            // the pipe that must receive fd zero.
            let closed = unsafe { libc::close(libc::STDIN_FILENO) };
            assert!(
                closed == 0 || last_errno() == libc::EBADF,
                "closing helper stdin: {}",
                std::io::Error::last_os_error()
            );
            let mut pipe = [-1; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            assert_eq!(pipe[0], 0, "closed stdin was not reused as pipe fd zero");
            let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
            assert!(open_max > 0, "invalid open-file descriptor ceiling");
            let open_max = libc::c_int::try_from(open_max).expect("descriptor ceiling fits c_int");

            close_inherited_fds(
                &[pipe[0], pipe[1], libc::STDOUT_FILENO, libc::STDERR_FILENO],
                open_max,
            );
            let sent = [0x5a_u8];
            let mut received = [0_u8];
            assert!(write_raw(pipe[1], &sent), "write through kept pipe");
            assert!(
                read_raw_exact(pipe[0], &mut received),
                "close_range closed kept fd zero"
            );
            assert_eq!(received, sent);
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn linux_close_range_preserves_a_kept_fd_zero() {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "linux_close_range_fd_zero_helper",
                    "--ignored",
                    "--nocapture",
                ])
                .env("UPSTROKE_CLOSE_RANGE_FD_ZERO_HELPER", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let output = command.output().expect("run fd-zero close-range helper");
            assert!(
                output.status.success(),
                "fd-zero helper failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        #[test]
        fn a_launch_cannot_enter_after_the_suspend_snapshot() {
            let state = Arc::new(Mutex::new(State {
                spawning: 0,
                groups: vec![RegisteredGroup {
                    pgid: 41,
                    signal_leases: 0,
                }],
                terminating: false,
                suspending: false,
                guard: Guard {
                    command_fd: -1,
                    ack_fd: -1,
                    _command_keepalive_fd: -1,
                    pid: -1,
                },
            }));
            let (groups, _) = begin_suspend(&state).expect("begin suspend transition");
            assert_eq!(&*groups, &[41]);

            let waiting = Arc::clone(&state);
            let (sent, received) = std::sync::mpsc::channel();
            let launch = thread::spawn(move || {
                // Exercise the production launch gate without fabricating a
                // second independent cleanup-reaper registry. Production has
                // one shared registry and serializes helper creation through
                // this claim; constructing a private Supervisor here violates
                // that invariant and makes Darwin FIFO inheritance part of a
                // synchronization test that never intended to cover it.
                claim_launch(&waiting).expect("launch after resume");
                sent.send(()).expect("report launch");
                release_launch(&waiting);
            });
            assert!(
                received.recv_timeout(Duration::from_millis(50)).is_err(),
                "a launch entered while the frozen process-group snapshot was active"
            );

            let resumed = end_suspend(&state);
            assert_eq!(&*resumed, &[41]);
            drop(resumed);
            drop(groups);
            received
                .recv_timeout(Duration::from_secs(2))
                .expect("launch released after resume");
            launch.join().expect("join launch");
            let locked = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert_eq!(locked.spawning, 0);
        }

        #[test]
        fn signal_snapshot_pins_a_group_until_delivery_finishes() {
            let state = Arc::new(Mutex::new(State {
                spawning: 0,
                groups: vec![RegisteredGroup {
                    pgid: 41,
                    signal_leases: 0,
                }],
                terminating: false,
                suspending: false,
                guard: Guard {
                    command_fd: -1,
                    ack_fd: -1,
                    _command_keepalive_fd: -1,
                    pid: -1,
                },
            }));
            let snapshot = groups_when_registered(&state, false).expect("group snapshot");
            {
                let mut locked = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                assert_eq!(locked.groups[0].signal_leases, 1);
                assert!(
                    !remove_unpinned_group(&mut locked, 41),
                    "finish exposed the group id while a signal snapshot held it"
                );
            }
            drop(snapshot);
            let mut locked = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(remove_unpinned_group(&mut locked, 41));
            assert!(locked.groups.is_empty());
        }

        #[test]
        fn helper_pipe_descriptors_are_close_on_exec() {
            let pipe = create_cloexec_pipe().expect("atomic close-on-exec pipe");
            for fd in pipe {
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
                assert!(flags >= 0, "read descriptor flags");
                assert_ne!(
                    flags & libc::FD_CLOEXEC,
                    0,
                    "helper descriptor was visible without close-on-exec"
                );
                close_fd(fd);
            }
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn linux_stat_parser_handles_spaces_and_closing_parentheses_in_comm() {
            let stat = "123 (reviewer ) helper) T 7 123 123 0 -1 0";
            assert_eq!(parse_linux_process_stat(stat), Some((123, b'T')));
            assert_eq!(parse_linux_stat_bytes(stat.as_bytes()), Some((123, b'T')));
            assert_eq!(parse_linux_process_stat("malformed"), None);
            assert_eq!(parse_linux_stat_bytes(b"malformed"), None);
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn a_vanished_linux_pid_is_not_an_incomplete_scanner_snapshot() {
            assert_eq!(read_linux_stat_raw(i32::MAX), LinuxStatSnapshot::Vanished);
        }

        #[test]
        fn a_zombie_only_group_is_quiescent_for_cleanup() {
            // SAFETY: the child performs only async-signal-safe syscalls and
            // exits immediately. The parent deliberately observes it without
            // reaping so the cleanup scanner sees a real zombie-only PGID.
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                let code = i32::from(unsafe { libc::setpgid(0, 0) } != 0);
                unsafe { libc::_exit(code) };
            }
            assert!(pid > 0, "fork failed: {}", std::io::Error::last_os_error());

            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                assert_eq!(result, 0, "waitid: {}", std::io::Error::last_os_error());
                if unsafe { info.si_pid() } == pid {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "child never became a zombie"
                );
                thread::sleep(Duration::from_millis(1));
            }

            // An unrelated process can disappear between `/proc` enumeration
            // and its stat read, making one conservative scanner snapshot
            // unknown. Cleanup retries that state; this regression must model
            // the same contract rather than requiring an unrealistically
            // quiescent runner on its first snapshot.
            let scan_deadline = std::time::Instant::now() + Duration::from_secs(2);
            let observed = loop {
                match group_has_non_zombie_members(pid) {
                    observed @ Some(_) => break observed,
                    None if std::time::Instant::now() < scan_deadline => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    None => break None,
                }
            };
            unsafe {
                let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            assert_eq!(observed, Some(false));
        }

        /// The program handed to `execv` is **always absolute**, and a bare name
        /// `PATH` cannot resolve is refused where there is still an error channel.
        ///
        /// `execv` does not search `PATH`; only `execvp` does, and `execvp` is not
        /// async-signal-safe, so a reaper may not call it. The production spelling
        /// is `runner::container::DOCKER_PROGRAM` — the bare name `docker`
        /// — so a reaper handed that name unresolved lists nothing, reclaims
        /// nothing, and reports success.
        ///
        /// Second field held constant: the private root and the incarnation are the
        /// same in every cell, so the only thing that moves is the **spelling of
        /// the program** — bare-and-resolvable, bare-and-absent, and a path.
        #[test]
        fn the_reaper_program_is_resolved_to_an_absolute_path_before_the_fork() {
            use crate::runner::container::census::ReaperContainerScope;
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt as _;
            use std::path::Path;

            const ROOT: &str = "/srv/upstroke-reaper-resolve/private";
            const INCARNATION: &str = "01KZTBBBBBBBBBBBBBBBBBBBBB";

            fn scope(program: &str) -> ReaperContainerScope {
                ReaperContainerScope::new(program, Path::new(ROOT), INCARNATION)
                    .expect("a well-formed scope")
            }
            fn execd(rendered: &ReaperContainers) -> std::path::PathBuf {
                std::path::PathBuf::from(OsStr::from_bytes(rendered.program.as_bytes()))
            }

            // (1) A bare name that `PATH` resolves. `git` rather than `docker`
            // because the property is about resolution and every machine that
            // builds this repository has git; `util::find_program_resolves_real_
            // tools_and_misses_fake_ones` is where that is already relied on.
            let rendered = render_container_argv(&scope("git")).expect("git is on PATH");
            let program = execd(&rendered);
            assert!(
                program.is_absolute(),
                "`execv` was handed `{}`, which it will not search `PATH` for",
                program.display()
            );
            assert!(
                program.is_file(),
                "`{}` is not a file on this machine",
                program.display()
            );
            assert_eq!(
                program.file_name(),
                Some(OsStr::new("git")),
                "the resolution found something other than the program it was asked for: {}",
                program.display()
            );
            // `argv[0]` is the resolved path too, so the string the child execs and
            // the string it reports itself as cannot drift apart.
            let argv0 = unsafe { std::ffi::CStr::from_ptr(rendered.ps_argv[0]) };
            assert_eq!(argv0.to_bytes(), program.as_os_str().as_bytes());

            // (2) A bare name `PATH` cannot resolve is refused, while there is
            // still somewhere to report it: a reaper has no error channel.
            let absent = scope("upstroke-definitely-not-a-real-docker");
            let message = match render_container_argv(&absent) {
                Ok(_) => panic!("an unresolvable bare program was accepted"),
                Err(error) => error.to_string(),
            };
            assert!(
                message.contains("PATH")
                    && message.contains("upstroke-definitely-not-a-real-docker"),
                "{message}"
            );
            // And the refusal reaches the caller that arms the reaper, which is the
            // only place with an error channel. Nothing is installed on this path,
            // so no other test in this process inherits a scope.
            assert!(
                set_container_reclaim_scope(Some(&absent)).is_err(),
                "arming accepted a scope whose program cannot be executed"
            );

            // (3) A name carrying a separator is a path and is used verbatim —
            // exactly `execvp`'s own rule, and what `execv` already does correctly.
            let rendered = render_container_argv(&scope("/usr/bin/docker")).expect("a path");
            assert_eq!(execd(&rendered), Path::new("/usr/bin/docker"));
            let rendered = render_container_argv(&scope("./docker")).expect("a relative path");
            assert_eq!(execd(&rendered), Path::new("./docker"));
        }

        /// A scratch directory, a recording `docker` stub, and the rendered
        /// argument vectors that name it.
        fn reaper_stub(tag: &str, script: &str) -> (std::path::PathBuf, ReaperContainers) {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = std::env::temp_dir().join(format!(
                "upstroke-reaper-rounds-{tag}-{}-{}",
                std::process::id(),
                crate::ulid::ulid()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch");
            let stub = dir.join("docker-stub");
            std::fs::write(&stub, script).expect("write the stub");
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                .expect("make the stub executable");
            // The stub finds its own scratch directory through `dirname $0`,
            // so nothing about this fixture depends on process-wide state and
            // two of these may run concurrently.
            let scope = crate::runner::container::census::ReaperContainerScope::new(
                &stub,
                std::path::Path::new("/srv/upstroke-reaper-rounds/private"),
                "01KZTAAAAAAAAAAAAAAAAAAAAA",
            )
            .expect("a scope");
            let rendered = render_container_argv(&scope).expect("argv");
            (dir, rendered)
        }

        /// Every line of the stub's log whose first word is `verb`, in order.
        fn logged(dir: &std::path::Path, verb: &str) -> Vec<String> {
            std::fs::read_to_string(dir.join("argv.log"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.split_whitespace().next() == Some(verb))
                .map(str::to_owned)
                .collect()
        }

        /// The reaper performs **as many rounds as the machine needs**, not a
        /// fixed number of them.
        ///
        /// The listing buffer is fixed at [`REAPER_PS_BUFFER`] and a
        /// `--no-trunc` id is 65 bytes with its newline, so one listing holds
        /// **126** ids. A round count of 8 therefore made the reaper's reach a
        /// silent **1,008** containers: a coordinator dying with 1,009 left one
        /// behind and the reaper reported the same success it reports on a
        /// clean machine. Twelve rounds is more than eight and few enough to
        /// run in a fraction of a second; what it measures is that the count is
        /// gone, not that the count is twelve.
        ///
        /// Second field held constant: exactly one container is listed in every
        /// round, so the number of ids per listing cannot be what ends the
        /// loop — only the number of rounds moves.
        #[test]
        fn the_reaper_performs_as_many_rounds_as_the_machine_needs() {
            const ROUNDS: usize = 12;
            let (dir, rendered) = reaper_stub(
                "unbounded",
                &format!(
                    "#!/bin/sh\n\
                     d=$(dirname \"$0\")\n\
                     printf '%s\\n' \"$*\" >> \"$d/argv.log\"\n\
                     case \"$1\" in\n\
                     ps) n=$(cat \"$d/round\" 2>/dev/null || echo 0); n=$((n+1)); \
                     echo \"$n\" > \"$d/round\"; \
                     [ \"$n\" -gt {ROUNDS} ] || printf '%064d\\n' \"$n\" ;;\n\
                     esac\n\
                     exit 0\n"
                ),
            );

            reclaim_labeled_containers(&rendered);

            let killed: std::collections::BTreeSet<String> = logged(&dir, "kill")
                .into_iter()
                .map(|line| line["kill ".len()..].to_owned())
                .collect();
            let removed: std::collections::BTreeSet<String> = logged(&dir, "rm")
                .into_iter()
                .map(|line| line["rm --force --volumes ".len()..].to_owned())
                .collect();
            // The expected ids come from the stub's own rule, written out here
            // rather than read back from what the reaper did.
            let expected: std::collections::BTreeSet<String> =
                (1..=ROUNDS).map(|n| format!("{n:064}")).collect();
            assert_eq!(
                killed,
                expected,
                "the reaper stopped early: it killed {} of {ROUNDS}",
                killed.len()
            );
            assert_eq!(removed, expected, "kill and rm did not settle the same set");
            assert!(
                logged(&dir, "ps").len() > ROUNDS,
                "{} listings for {ROUNDS} rounds plus the empty one that ends the loop",
                logged(&dir, "ps").len()
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A listing **larger than the buffer** is not silently truncated: the
        /// ids that did not fit are settled by a later round.
        ///
        /// This is the other half of the 126 x 8 arithmetic, and the two are a
        /// product: a fixture that varied only the number of rounds would not
        /// notice a buffer that dropped what it could not hold, and one that
        /// varied only the number of ids would not notice a round count. 130 is
        /// the smallest number that crosses the boundary — 130 x 65 = 8,450
        /// bytes into an 8,192-byte buffer — so the first listing is cut inside
        /// an id, which is the case a parser is most likely to get wrong.
        ///
        /// Second field held constant: the stub removes exactly what it is
        /// told to remove and invents nothing, so the only thing that moves
        /// between rounds is how much of the set is left.
        #[test]
        fn the_reaper_settles_more_containers_than_one_listing_holds() {
            const CONTAINERS: usize = 130;
            let (dir, rendered) = reaper_stub(
                "over-buffer",
                "#!/bin/sh\n\
                 d=$(dirname \"$0\")\n\
                 printf '%s\\n' \"$*\" >> \"$d/argv.log\"\n\
                 case \"$1\" in\n\
                 ps) ls \"$d/ids\" 2>/dev/null ;;\n\
                 rm) rm -f \"$d/ids/$4\" ;;\n\
                 esac\n\
                 exit 0\n",
            );
            std::fs::create_dir_all(dir.join("ids")).expect("the id set");
            let expected: std::collections::BTreeSet<String> = (1..=CONTAINERS)
                .map(|n| format!("{n:064}"))
                .inspect(|id| std::fs::write(dir.join("ids").join(id), "").expect("an id"))
                .collect();
            assert_eq!(expected.len(), CONTAINERS);
            const {
                assert!(
                    CONTAINERS * 65 > REAPER_PS_BUFFER,
                    "the fixture must not fit in one listing"
                );
            }

            reclaim_labeled_containers(&rendered);

            let killed: std::collections::BTreeSet<String> = logged(&dir, "kill")
                .into_iter()
                .map(|line| line["kill ".len()..].to_owned())
                .collect();
            let removed: std::collections::BTreeSet<String> = logged(&dir, "rm")
                .into_iter()
                .map(|line| line["rm --force --volumes ".len()..].to_owned())
                .collect();
            assert_eq!(
                killed,
                expected,
                "{} of {CONTAINERS} containers were killed; the ids past the end of the first \
                 listing were dropped rather than settled by a later round",
                killed.len()
            );
            assert_eq!(removed, expected);
            assert_eq!(
                std::fs::read_dir(dir.join("ids"))
                    .expect("the id set")
                    .count(),
                0,
                "the stub still holds containers the reaper never removed"
            );
            assert!(
                logged(&dir, "ps").len() >= 3,
                "one listing cannot hold 130 ids"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A runtime that keeps answering with the **same listing** ends the
        /// loop on the second round, and does not repeat for ever.
        ///
        /// This is the guard the round count used to be, in its real form. The
        /// reaper holds R28 — the shared cleanup hold the next coordinator waits
        /// on — so a loop that could not end would turn "docker is wedged" into
        /// "no run on this machine can ever start again".
        ///
        /// Second field held constant: the id set, which never changes; only
        /// the number of times the reaper is willing to ask about it moves.
        #[test]
        fn a_runtime_that_keeps_answering_the_same_listing_ends_the_loop() {
            // The stub answers with the same two ids twice and then with
            // nothing. The third listing is what makes this fixture finite: an
            // implementation with **no** no-progress guard still terminates
            // here, and is caught by the counts rather than by a hang, because
            // a test that measures a missing bound by hanging measures nothing.
            let (dir, rendered) = reaper_stub(
                "no-progress",
                "#!/bin/sh\n\
                 d=$(dirname \"$0\")\n\
                 printf '%s\\n' \"$*\" >> \"$d/argv.log\"\n\
                 case \"$1\" in\n\
                 ps) n=$(cat \"$d/round\" 2>/dev/null || echo 0); n=$((n+1)); \
                 echo \"$n\" > \"$d/round\"; \
                 [ \"$n\" -gt 2 ] || { printf '%064d\\n' 1; printf '%064d\\n' 2; } ;;\n\
                 esac\n\
                 exit 0\n",
            );

            reclaim_labeled_containers(&rendered);

            // Two listings: the first is acted on, the second is recognised as
            // the same answer and ends the loop **there** — the third listing,
            // which the stub is willing to answer, is never asked for. Each
            // container was attempted exactly once, so nothing is retried
            // against a runtime that has already refused to remove it.
            assert_eq!(
                logged(&dir, "ps").len(),
                2,
                "a repeated listing was acted on again: {:?}",
                logged(&dir, "ps")
            );
            assert_eq!(logged(&dir, "kill").len(), 2, "{:?}", logged(&dir, "kill"));
            assert_eq!(logged(&dir, "rm").len(), 2, "{:?}", logged(&dir, "rm"));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Test-only convenience entry. Production passes both sites explicitly.
    pub(crate) fn run_with_timeout(
        command: Command,
        stdin_data: &str,
        timeout: Duration,
    ) -> Result<ProcessOutput, UpstrokeError> {
        run_with_timeout_at(
            ProcessSite::Spawn,
            ProcessSite::Terminate,
            command,
            stdin_data.as_bytes(),
            timeout,
            &mut NoHooks,
        )
    }

    // **Out of line, in `proc/test_support/readiness.rs`.** The primitives are
    // ~440 lines of protocol that three modules' fixtures depend on, and they
    // were the only thing in this file with no relationship to subprocess
    // supervision. Moving them gives them their own module doc and their own
    // stated lint level -- a Rust lint level is scoped by the module tree
    // rather than by the file, so an out-of-line child inherits this file's
    // `#![allow]` unless it says otherwise, which is `PR6-LANEF-004`.
    //
    // The path does not change: this declaration keeps
    // `crate::agent::proc::test_support::readiness` resolving exactly as it
    // did, for `workspace`, `rundir` and the witnesses below.
    //
    // **Declared without a `#[cfg(test)]` of its own**, because it inherits one:
    // `test_support` above carries it, so the file is compiled only under
    // `cfg(test)` and `effects::census_domain` resolves it as a whole-file test
    // module through that inline ancestry rather than through an attribute
    // written here.
    pub(crate) mod readiness;
}

#[cfg(test)]
mod tests;
