//! Subprocess supervision: run a command, feed stdin, drain both pipes
//! concurrently (required on Windows — a full pipe buffer deadlocks a child
//! that is still writing), and enforce a wall-clock timeout. Std-only; the
//! tokio scheduler arrives in v0.2.
//!
//! Windows subtleties this module owns: `.cmd` shims (npm installs) mean the
//! direct child is `cmd.exe`, so timeouts must kill the process *tree*
//! (`taskkill /T`), and any orphan that inherited a pipe handle must not be
//! able to stall the drain — readers accumulate into shared buffers that are
//! snapshotted after a bounded grace instead of joined unconditionally.
//!
//! Unix subtleties are the mirror image: each invocation gets an isolated
//! process group so a timeout can kill every descendant, but that isolation
//! also stops terminal interrupts reaching the child automatically. A tiny
//! process-wide signal monitor below preserves inherited ignored and custom
//! handlers,
//! coordinates SIGINT/SIGTERM/SIGHUP/SIGQUIT termination, and proxies terminal
//! suspension/continuation. It waits out any spawn-registration race, blocks
//! launches across a suspension transition, and uses a descriptor-scrubbed
//! guard process to close the last signal-to-stop race. A separate cleanup
//! reaper survives even an uncatchable Tactus SIGKILL. Together they kill every
//! active process group before ownership is released and stop every active
//! group whenever Tactus stops. Run ownership therefore cannot be handed to a
//! resume -- or appear suspended -- while an isolated agent is running.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::TactusError;

#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// Exit code if the process exited normally; `None` when killed (timeout)
    /// or terminated by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Wall clock from spawn to process exit (not including pipe drain).
    pub duration: Duration,
    pub timed_out: bool,
}

/// How long to keep draining pipes after the process is gone. Normally EOF is
/// immediate; the grace only caps the pathological case of an orphaned
/// grandchild still holding a write handle.
const DRAIN_GRACE_EXIT: Duration = Duration::from_secs(2);
const DRAIN_GRACE_KILL: Duration = Duration::from_millis(500);

/// Run `command`, writing `stdin_data` to the child's stdin, with a hard
/// wall-clock timeout. On timeout the child's process tree is killed and the
/// partial output captured so far is returned with `timed_out = true` (§14:
/// timeout is an attempt failure with the partial transcript as feedback).
pub fn run_with_timeout(
    mut command: Command,
    stdin_data: &str,
    timeout: Duration,
) -> Result<ProcessOutput, TactusError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Enter before `spawn`: if an interrupt arrives in the narrow interval
    // between creating the child and learning its pid, the signal monitor
    // waits for this registration rather than terminating Tactus first and
    // orphaning the new process group.
    #[cfg(unix)]
    let mut termination = termination::Supervisor::begin()?;
    #[cfg(unix)]
    termination.prepare(&mut command);

    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| TactusError::Agent {
        message: format!(
            "failed to spawn `{}`: {e}",
            command.get_program().to_string_lossy()
        ),
    })?;
    #[cfg(unix)]
    if let Err(error) = termination.register(child.id()) {
        // Drop the pre-exec reaper first: it still has an anchor pinning this
        // child's group identity and will kill every member before returning.
        drop(termination);
        kill_tree(&mut child);
        return Err(error);
    }

    // Feed stdin from its own thread: the child may not read stdin until it
    // has written output, and this thread must not block the pipe drains.
    let stdin_bytes = stdin_data.as_bytes().to_vec();
    let stdin_handle = child.stdin.take();
    let stdin_thread = thread::spawn(move || {
        if let Some(mut pipe) = stdin_handle {
            // A child that exits without reading stdin breaks the pipe; that
            // is its prerogative, not an error.
            let _ = pipe.write_all(&stdin_bytes);
        }
    });

    let stdout_drain = child.stdout.take().map(Drain::start);
    let stderr_drain = child.stderr.take().map(Drain::start);

    let mut timed_out = false;
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
                let status = child.wait().map_err(|e| TactusError::Agent {
                    message: format!("reaping agent process: {e}"),
                })?;
                break status.code();
            }
            Ok(false) => {
                if started.elapsed() >= timeout {
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
                return Err(TactusError::Agent {
                    message: format!("waiting on agent process: {e}"),
                });
            }
        }
    };
    #[cfg(not(unix))]
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if started.elapsed() >= timeout {
                    timed_out = true;
                    kill_tree(&mut child);
                    break None;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                kill_tree(&mut child);
                return Err(TactusError::Agent {
                    message: format!("waiting on agent process: {e}"),
                });
            }
        }
    };
    let duration = started.elapsed();

    let grace = if timed_out {
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
    let stdout = stdout_drain.map(|d| d.collect(grace)).unwrap_or_default();
    let stderr = stderr_drain.map(|d| d.collect(grace)).unwrap_or_default();

    Ok(ProcessOutput {
        code,
        stdout,
        stderr,
        duration,
        timed_out,
    })
}

/// Kill the whole process tree. Killing only the direct child is not enough
/// when it is a `cmd.exe` shim: the real agent process would survive, keep
/// running, and keep the pipes open.
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid])
            .output();
    }
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `run_with_timeout` put this child in a new process group
        // whose id is the child's pid. A negative pid targets that group only.
        let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
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
    Ok(unsafe { info.si_pid() } != 0)
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
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use crate::error::TactusError;

    static PENDING_TERMINATION: AtomicI32 = AtomicI32::new(0);
    static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);
    static CONTINUE_REQUESTED: AtomicBool = AtomicBool::new(false);
    static SUSPEND_ARMED: AtomicBool = AtomicBool::new(false);
    static GUARD_COMMAND_FD: AtomicI32 = AtomicI32::new(-1);
    static GUARD_WAKE_FD: AtomicI32 = AtomicI32::new(-1);
    static STATE: OnceLock<Result<Arc<Mutex<State>>, String>> = OnceLock::new();

    const GUARD_READY: u8 = 0x91;
    const GUARD_ARM: u8 = 0xa1;
    const GUARD_STOP: u8 = 0xb1;
    const GUARD_STOPPED: u8 = 0xb2;
    const GUARD_CANCELLED: u8 = 0xc1;
    const GUARD_DISARM: u8 = 0xd1;
    const HANDLE_SIGINT: u8 = 1 << 0;
    const HANDLE_SIGTERM: u8 = 1 << 1;
    const HANDLE_SIGHUP: u8 = 1 << 2;
    const HANDLE_SIGQUIT: u8 = 1 << 3;
    const REAPER_READY: u8 = 0x81;
    const REAPER_REGISTER: u8 = 0x82;
    const REAPER_CLEANUP: u8 = 0x83;
    const REAPER_OK: u8 = 0x84;
    const REAPER_FAIL: u8 = 0x85;

    #[derive(Clone, Copy)]
    struct SignalPolicy {
        termination_mask: u8,
        guard_wake_mask: u8,
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
    }

    struct State {
        /// Supervisors that entered before spawn but have not registered a pid.
        spawning: usize,
        /// Active isolated process-group ids.
        groups: Vec<i32>,
        /// Set by the monitor before it kills groups. No later spawn may begin.
        terminating: bool,
        /// Set before a suspend snapshot and cleared only after continuation.
        /// New launches wait outside the lock for the complete transition.
        suspending: bool,
        guard: Guard,
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
    }

    impl Supervisor {
        pub(super) fn begin() -> Result<Self, TactusError> {
            Self::begin_with_state(shared_state()?)
        }

        fn begin_with_state(state: Arc<Mutex<State>>) -> Result<Self, TactusError> {
            loop {
                let mut locked = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if locked.terminating || PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {
                    return Err(TactusError::Agent {
                        message: "process launch interrupted by a termination signal".to_owned(),
                    });
                }
                if !locked.suspending && locked.spawning == 0 {
                    locked.spawning = locked.spawning.saturating_add(1);
                    drop(locked);
                    let reaper = match spawn_reaper() {
                        Ok(reaper) => reaper,
                        Err(message) => {
                            let mut locked = state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            locked.spawning = locked.spawning.saturating_sub(1);
                            return Err(TactusError::Agent { message });
                        }
                    };
                    return Ok(Self {
                        state,
                        phase: Phase::Spawning,
                        reaper,
                    });
                }
                drop(locked);
                thread::sleep(Duration::from_millis(1));
            }
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

        pub(super) fn register(&mut self, pid: u32) -> Result<(), TactusError> {
            let pgid = i32::try_from(pid).map_err(|_| TactusError::Agent {
                message: format!("agent pid {pid} cannot be represented as a Unix process group"),
            })?;
            let mut locked = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locked.spawning = locked.spawning.saturating_sub(1);
            locked.groups.push(pgid);
            self.phase = Phase::Group(pgid);
            Ok(())
        }

        pub(super) fn finish(&mut self) -> Result<(), TactusError> {
            let Phase::Group(pgid) = self.phase else {
                return Ok(());
            };
            // `cleanup` consumes and closes the reaper's raw descriptors.
            // Change phase first so an error return followed by Drop can never
            // transact on—or close—descriptor numbers another thread may
            // already have reused.
            self.phase = Phase::Finished;
            if !self.reaper.cleanup(pgid) {
                let _ = PENDING_TERMINATION.compare_exchange(
                    0,
                    libc::SIGTERM,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                return Err(TactusError::Agent {
                    message: format!(
                        "Unix cleanup reaper failed while settling process group {pgid}"
                    ),
                });
            }
            let mut locked = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = locked
                .groups
                .iter()
                .position(|candidate| *candidate == pgid)
            {
                locked.groups.swap_remove(index);
            }
            Ok(())
        }
    }

    impl Drop for Supervisor {
        fn drop(&mut self) {
            match self.phase {
                Phase::Spawning => {
                    self.reaper.cancel();
                    let mut locked = self
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    locked.spawning = locked.spawning.saturating_sub(1);
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

    fn shared_state() -> Result<Arc<Mutex<State>>, TactusError> {
        match STATE.get_or_init(install) {
            Ok(state) => Ok(Arc::clone(state)),
            Err(message) => Err(TactusError::Agent {
                message: message.clone(),
            }),
        }
    }

    fn install() -> Result<Arc<Mutex<State>>, String> {
        let mut policy = SignalPolicy {
            termination_mask: 0,
            guard_wake_mask: 0,
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
        policy.job_control = disposition(libc::SIGTSTP)? == SignalDisposition::Default
            && disposition(libc::SIGCONT)? == SignalDisposition::Default;

        let guard = spawn_guard(policy)?;
        let state = Arc::new(Mutex::new(State {
            spawning: 0,
            groups: Vec::new(),
            terminating: false,
            suspending: false,
            guard,
        }));
        let monitored = Arc::clone(&state);
        thread::Builder::new()
            .name("tactus-signal-monitor".to_owned())
            .spawn(move || monitor(monitored))
            .map_err(|error| format!("starting Unix signal monitor: {error}"))?;

        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            // Preserve every launcher-owned policy. POSIX carries SIG_IGN
            // across exec (`nohup` relies on it), while an embedding host may
            // have installed a custom in-process handler before calling us.
            if policy.handles_termination(signal) {
                install_handler(signal)?;
            }
        }

        // Job-control proxying is useful only as a pair: if a launcher
        // explicitly ignored either half, preserve that policy rather than
        // stopping children we cannot reliably continue (or vice versa).
        if policy.job_control {
            install_handler(libc::SIGTSTP)?;
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

    fn install_handler(signal: libc::c_int) -> Result<(), String> {
        // SAFETY: `record_signal` has the C ABI and performs only lock-free
        // atomic operations. The empty mask and SA_RESTART keep unrelated
        // syscalls from being exposed to the implementation detail that a
        // monitor thread, rather than the handler, owns process-group work.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = record_signal as *const () as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
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
            libc::SIGTSTP => SUSPEND_REQUESTED.store(true, Ordering::SeqCst),
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
                // terminate Tactus with the original signal; `_exit` is a
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
                // cannot keep spending while its visibly foreground Tactus
                // parent is suspended. SIGCONT below releases the same groups.
                if !stop_groups(&groups) {
                    let groups = end_suspend(&state);
                    if PENDING_TERMINATION.load(Ordering::SeqCst) == 0 {
                        signal_groups(&groups, libc::SIGCONT);
                    }
                    continue;
                }

                // The guard remains runnable while Tactus is stopped. It
                // serializes a late continuation/termination with the actual
                // SIGSTOP and acknowledges only after a genuine resume. That
                // closes the final flag-check-to-stop interval.
                SUSPEND_ARMED.store(true, Ordering::SeqCst);
                if !guard.arm() {
                    SUSPEND_ARMED.store(false, Ordering::SeqCst);
                    let _ = end_suspend(&state);
                    let _ = PENDING_TERMINATION.compare_exchange(
                        0,
                        libc::SIGTERM,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
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
                        let _ = PENDING_TERMINATION.compare_exchange(
                            0,
                            libc::SIGTERM,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
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

    fn groups_when_registered(state: &Arc<Mutex<State>>, terminating: bool) -> Option<Vec<i32>> {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if locked.spawning != 0 {
            return None;
        }
        if terminating {
            locked.terminating = true;
        }
        Some(locked.groups.clone())
    }

    fn begin_suspend(state: &Arc<Mutex<State>>) -> Option<(Vec<i32>, Guard)> {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if locked.spawning != 0 || locked.suspending || locked.terminating {
            return None;
        }
        locked.suspending = true;
        Some((locked.groups.clone(), locked.guard))
    }

    fn end_suspend(state: &Arc<Mutex<State>>) -> Vec<i32> {
        let mut locked = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locked.suspending = false;
        locked.groups.clone()
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
        let mut command = [-1; 2];
        let mut ack = [-1; 2];
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
        let cleanup_delay_ms = std::env::var("TACTUS_TEST_CLEANUP_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        #[cfg(not(test))]
        let cleanup_delay_ms = 0;
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if open_max <= 0 {
            return Err("reading the Unix open-file descriptor ceiling".to_owned());
        }
        let open_max = libc::c_int::try_from(open_max)
            .map_err(|_| "Unix open-file descriptor ceiling exceeds c_int".to_owned())?;
        if unsafe { libc::pipe(command.as_mut_ptr()) } != 0 {
            return Err(format!(
                "creating Unix cleanup-reaper command pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { libc::pipe(ack.as_mut_ptr()) } != 0 {
            close_fd(command[0]);
            close_fd(command[1]);
            return Err(format!(
                "creating Unix cleanup-reaper acknowledgement pipe: {}",
                std::io::Error::last_os_error()
            ));
        }

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
            // uncatchable kill of Tactus's foreground job must not also kill
            // the process that owns its final agent cleanup.
            if unsafe { libc::setpgid(0, 0) } != 0 {
                unsafe { libc::_exit(1) };
            }
            close_inherited_fds(&[command[0], ack[1]], open_max);
            if !lock_cleanup_paths(&cleanup_paths) {
                unsafe { libc::_exit(1) };
            }
            reaper_loop(command[0], ack[1], open_max, cleanup_delay_ms);
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
        if !set_close_on_exec(command[0])
            || !set_close_on_exec(command[1])
            || !set_close_on_exec(ack[0])
        {
            for fd in [command[0], command[1], ack[0]] {
                close_fd(fd);
            }
            unsafe {
                let _ = libc::kill(pid, libc::SIGKILL);
                let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            return Err("configuring Unix cleanup-reaper descriptors".to_owned());
        }
        let reaper = Reaper {
            command_fd: command[1],
            ack_fd: ack[0],
            _command_keepalive_fd: command[0],
            pid,
        };
        if read_guard_ack(ack[0], Duration::from_secs(2)) != Some(REAPER_READY) {
            reaper.cancel();
            return Err("Unix cleanup reaper did not initialize".to_owned());
        }
        Ok(reaper)
    }

    fn install_reaper_dispositions() -> bool {
        unsafe {
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
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        open_max: libc::c_int,
        cleanup_delay_ms: u64,
    ) -> ! {
        let mut pgid = 0_i32;
        let mut anchor = 0_i32;
        if !write_raw(ack_fd, &[REAPER_READY]) {
            unsafe { libc::_exit(1) };
        }
        loop {
            let mut frame = [0_u8; 5];
            if !read_raw_exact(command_fd, &mut frame) {
                if pgid > 0 {
                    cleanup_reaper_group(pgid, anchor, cleanup_delay_ms);
                }
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
                _ => false,
            };
            if !write_raw(ack_fd, &[if accepted { REAPER_OK } else { REAPER_FAIL }]) {
                if pgid > 0 {
                    cleanup_reaper_group(pgid, anchor, cleanup_delay_ms);
                }
                unsafe { libc::_exit(0) };
            }
        }
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
        let mut command = [-1; 2];
        let mut ack = [-1; 2];
        let mut wake = [-1; 2];
        // Resolve the descriptor ceiling before fork: sysconf may take libc
        // locks, whereas the multithreaded child may call only async-safe
        // primitives until it enters the guard loop.
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        if open_max <= 0 {
            return Err("reading the Unix open-file descriptor ceiling".to_owned());
        }
        let open_max = libc::c_int::try_from(open_max)
            .map_err(|_| "Unix open-file descriptor ceiling exceeds c_int".to_owned())?;
        // SAFETY: both arrays provide the two writable descriptors required by
        // `pipe`; every error path below closes descriptors already created.
        if unsafe { libc::pipe(command.as_mut_ptr()) } != 0 {
            return Err(format!(
                "creating Unix job-control command pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { libc::pipe(ack.as_mut_ptr()) } != 0 {
            close_fd(command[0]);
            close_fd(command[1]);
            return Err(format!(
                "creating Unix job-control acknowledgement pipe: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { libc::pipe(wake.as_mut_ptr()) } != 0
            || !set_nonblocking(wake[0])
            || !set_nonblocking(wake[1])
        {
            for fd in [command[0], command[1], ack[0], ack[1], wake[0], wake[1]] {
                close_fd(fd);
            }
            return Err(format!(
                "creating Unix job-control wake pipe: {}",
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
            for fd in [command[0], command[1], ack[0], ack[1], wake[0], wake[1]] {
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
            close_fd(command[1]);
            close_fd(ack[0]);
            close_inherited_fds(&[command[0], ack[1], wake[0], wake[1]], open_max);
            guard_loop(unsafe { libc::getppid() }, command[0], ack[1], wake[0]);
        }

        GUARD_WAKE_FD.store(-1, Ordering::SeqCst);
        close_fd(ack[1]);
        close_fd(wake[0]);
        close_fd(wake[1]);
        if !set_close_on_exec(command[0])
            || !set_close_on_exec(command[1])
            || !set_close_on_exec(ack[0])
            || !set_nonblocking(command[1])
        {
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
        };
        if guard.read_ack() != Some(GUARD_READY) {
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
        GUARD_COMMAND_FD.store(command[1], Ordering::SeqCst);
        Ok(guard)
    }

    fn guard_loop(
        parent: libc::pid_t,
        command_fd: libc::c_int,
        ack_fd: libc::c_int,
        wake_fd: libc::c_int,
    ) -> ! {
        if !write_byte(ack_fd, GUARD_READY) {
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
            let polled = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, -1) };
            if polled < 0 && !last_errno_is_interrupted() {
                unsafe { libc::_exit(1) };
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
                            // original Tactus process is gone.
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
                // unrelated process. Reparenting proves the original Tactus
                // process is gone even if its numeric pid has been reused.
                if unsafe { libc::getppid() } != parent {
                    unsafe { libc::_exit(0) };
                }
                // SAFETY: a positive pid targets only the Tactus parent. A
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

    fn install_guard_dispositions(policy: SignalPolicy) -> bool {
        // The guard stays in the foreground process group but cannot join the
        // stop: it ignores SIGTSTP and records every transition that must wake
        // a parent already stopped by the guard. SIGSTOP itself targets only
        // the parent pid.
        unsafe {
            // A library host may have blocked these signals on the thread
            // that first invoked Tactus. The guard is an isolated relay, not
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
                // parent when Tactus cannot safely proxy the pair. The private
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
                    // default Tactus-owned termination signal instead.
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
            let last = next_keep.map_or(u32::MAX, |fd| fd.saturating_sub(1));
            if first <= last {
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
        // Process enumeration can race an unrelated process exiting. Retry a
        // few complete snapshots, but refuse before launching an agent when
        // the platform capability is persistently absent (for example a
        // Linux container without a mounted/readable procfs).
        for _ in 0..5 {
            match group_has_non_zombie_members(own_group) {
                Some(true) => return Ok(()),
                Some(false) => {
                    return Err(format!(
                        "Unix process-group scanner did not find the current group {own_group}"
                    ));
                }
                None => thread::sleep(Duration::from_millis(1)),
            }
        }
        Err("Unix process-group scanner is unavailable; refusing to launch an agent whose cleanup could not be verified".to_owned())
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
                        Some((candidate, state))
                            if candidate == pgid && !matches!(state, b'Z' | b'X' | b'x') =>
                        {
                            close_fd(directory);
                            return Some(true);
                        }
                        Some(_) => {}
                        // A process may have disappeared between enumeration
                        // and stat; the next scan resolves that ordinary race.
                        // Treat every unreadable or malformed snapshot as
                        // unknown for this pass rather than guessing it was an
                        // unrelated/dead member and releasing cleanup early.
                        None => {
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
    fn read_linux_stat_raw(pid: i32) -> Option<(i32, u8)> {
        let mut path = [0_u8; 64];
        let prefix = b"/proc/";
        path[..prefix.len()].copy_from_slice(prefix);
        let mut end = prefix.len();
        end += write_decimal(pid, &mut path[end..])?;
        let suffix = b"/stat\0";
        path.get_mut(end..end + suffix.len())?
            .copy_from_slice(suffix);
        let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let mut stat = [0_u8; 2_048];
        let count = loop {
            let read = unsafe { libc::read(fd, stat.as_mut_ptr().cast(), stat.len()) };
            if read < 0 && last_errno_is_interrupted() {
                continue;
            }
            break read;
        };
        close_fd(fd);
        if count <= 0 {
            return None;
        }
        parse_linux_stat_bytes(&stat[..count as usize])
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
                // As on Linux, a disappearing pid is resolved by the next
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

    fn set_close_on_exec(fd: libc::c_int) -> bool {
        // SAFETY: fcntl only updates the supplied live descriptor. The caller
        // treats failure as fatal because agent descendants must never retain
        // the guard's control pipes across exec.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                return libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == 0;
            }
        }
        false
    }

    fn set_nonblocking(fd: libc::c_int) -> bool {
        // Signal handlers may write this descriptor. Nonblocking mode makes a
        // dead or unresponsive guard fail closed instead of wedging Tactus in
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
            // negative id targets that group and never Tactus's group.
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
        // direct child that Tactus can wait on.
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        static REAPED_CHILD_STOP: AtomicBool = AtomicBool::new(false);

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
            if std::env::var_os("TACTUS_SIGCHLD_REAPER_HELPER").is_none() {
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
                .env("TACTUS_SIGCHLD_REAPER_HELPER", "1")
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

        #[test]
        fn a_launch_cannot_enter_after_the_suspend_snapshot() {
            let state = Arc::new(Mutex::new(State {
                spawning: 0,
                groups: vec![41],
                terminating: false,
                suspending: false,
                guard: Guard {
                    command_fd: -1,
                    ack_fd: -1,
                    _command_keepalive_fd: -1,
                },
            }));
            let (groups, _) = begin_suspend(&state).expect("begin suspend transition");
            assert_eq!(groups, vec![41]);

            let waiting = Arc::clone(&state);
            let (sent, received) = std::sync::mpsc::channel();
            let launch = thread::spawn(move || {
                let supervisor =
                    Supervisor::begin_with_state(waiting).expect("launch after resume");
                sent.send(()).expect("report launch");
                supervisor
            });
            assert!(
                received.recv_timeout(Duration::from_millis(50)).is_err(),
                "a launch entered while the frozen process-group snapshot was active"
            );

            assert_eq!(end_suspend(&state), vec![41]);
            received
                .recv_timeout(Duration::from_secs(2))
                .expect("launch released after resume");
            drop(launch.join().expect("join launch"));
            let locked = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert_eq!(locked.spawning, 0);
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

            let observed = group_has_non_zombie_members(pid);
            unsafe {
                let _ = libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            assert_eq!(observed, Some(false));
        }
    }
}

/// A pipe reader whose buffer can be snapshotted without joining the thread,
/// so an orphan holding the write end can never stall the supervisor.
struct Drain {
    buf: Arc<Mutex<Vec<u8>>>,
    handle: thread::JoinHandle<()>,
}

impl Drain {
    fn start<R: Read + Send + 'static>(mut pipe: R) -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::clone(&buf);
        let handle = thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => match writer.lock() {
                        Ok(mut guard) => guard.extend_from_slice(&chunk[..n]),
                        Err(poisoned) => poisoned.into_inner().extend_from_slice(&chunk[..n]),
                    },
                }
            }
        });
        Self { buf, handle }
    }

    /// Wait up to `grace` for EOF, then snapshot whatever arrived. A reader
    /// abandoned here exits on its own when the last write handle closes.
    fn collect(self, grace: Duration) -> String {
        let deadline = Instant::now() + grace;
        while !self.handle.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if self.handle.is_finished() {
            let _ = self.handle.join();
        }
        let snapshot = match self.buf.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        String::from_utf8_lossy(&snapshot).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Windows-first-class: exercise the supervisor through cmd.exe, which is
    // always present there; use sh on everything else.
    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn captures_stdout_and_exit_code() {
        let out = run_with_timeout(shell("echo hello"), "", Duration::from_secs(30))
            .expect("spawn shell");
        assert_eq!(out.code, Some(0));
        assert!(out.stdout.contains("hello"));
        assert!(!out.timed_out);
    }

    #[test]
    fn nonzero_exit_is_reported_not_an_error() {
        let out =
            run_with_timeout(shell("exit 3"), "", Duration::from_secs(30)).expect("spawn shell");
        assert_eq!(out.code, Some(3));
    }

    #[test]
    fn stdin_reaches_the_child() {
        let script = if cfg!(windows) { "findstr ping" } else { "cat" };
        let out = run_with_timeout(shell(script), "ping pong\n", Duration::from_secs(30))
            .expect("spawn shell");
        assert!(out.stdout.contains("ping"), "stdout: {}", out.stdout);
    }

    #[test]
    fn timeout_kills_the_process_tree_quickly() {
        // Through the shell, the sleeper is a grandchild — exactly the
        // claude.cmd shim shape this module must handle.
        let script = if cfg!(windows) {
            "ping -n 30 127.0.0.1 > NUL"
        } else {
            "sleep 30"
        };
        let started = Instant::now();
        let out =
            run_with_timeout(shell(script), "", Duration::from_millis(300)).expect("spawn shell");
        assert!(out.timed_out);
        assert!(out.code.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "supervisor returned promptly, no orphan stall: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_a_background_grandchild_before_it_can_escape() {
        let marker = std::env::temp_dir().join(format!(
            "tactus-proc-tree-{}-{}.marker",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_file(&marker);

        let mut command = shell("(sleep 1; printf leaked > \"$TACTUS_MARKER\") & wait");
        command.env("TACTUS_MARKER", &marker);
        let out = run_with_timeout(command, "", Duration::from_millis(200)).expect("spawn shell");
        assert!(out.timed_out);

        thread::sleep(Duration::from_millis(1300));
        let leaked = marker.exists();
        let _ = std::fs::remove_file(&marker);
        assert!(
            !leaked,
            "the timed-out process group's background grandchild survived"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_direct_exit_still_kills_detached_group_members() {
        let marker = std::env::temp_dir().join(format!(
            "tactus-proc-detached-{}-{}.marker",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&marker);
        let mut command = shell(
            "(sleep 1; printf leaked > \"$TACTUS_MARKER\") \
             </dev/null >/dev/null 2>&1 & exit 0",
        );
        command.env("TACTUS_MARKER", &marker);
        let output = run_with_timeout(command, "", Duration::from_secs(10)).expect("spawn shell");
        assert_eq!(output.code, Some(0));

        thread::sleep(Duration::from_millis(1300));
        let leaked = marker.exists();
        let _ = std::fs::remove_file(&marker);
        assert!(
            !leaked,
            "a detached descendant outlived the successful command"
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "subprocess helper"]
    fn terminal_progress_worker_helper() {
        if std::env::var_os("TACTUS_SIGNAL_WORKER").is_none() {
            return;
        }
        let ready = std::env::var_os("TACTUS_READY").expect("ready path");
        let marker = std::env::var_os("TACTUS_MARKER").expect("marker path");
        let finish = std::env::var_os("TACTUS_FINISH").expect("finish path");
        let pid = unsafe { libc::getpid() };
        let pgid = unsafe { libc::getpgrp() };
        std::fs::write(ready, format!("{pid} {pgid} {pid} {pgid}")).expect("worker ready");
        let mut progress = 0_u64;
        while !std::path::Path::new(&finish).exists() {
            progress += 1;
            std::fs::write(&marker, progress.to_string()).expect("worker progress");
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Subprocess entry point for the Unix signal-supervision tests below.
    /// Ignored in ordinary test discovery; the parent test invokes only this
    /// case in a fresh process because the expected outcome is SIGINT.
    #[cfg(unix)]
    #[test]
    #[ignore = "subprocess helper"]
    fn terminal_interrupt_helper() {
        if std::env::var_os("TACTUS_SIGNAL_HELPER").is_none() {
            return;
        }
        // SIGQUIT normally requests a core dump. Disable it in this disposable
        // helper so the regression observes supervision semantics without
        // invoking a host crash reporter (notably ReportCrash on macOS).
        let no_core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: this changes only the current disposable helper before it
        // launches either the signal monitor or the supervised command.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &no_core) }, 0);
        let _cleanup_lock = std::env::var_os("TACTUS_CLEANUP_PUBLIC").map(|public| {
            let public = std::path::PathBuf::from(public);
            std::fs::create_dir_all(&public).expect("cleanup-lock run directory");
            crate::rundir::RunLock::acquire(&public).expect("cleanup-lock helper takes run")
        });
        let _cleanup_scope = _cleanup_lock
            .as_ref()
            .map(crate::rundir::RunLock::enter_cleanup_scope);
        if std::env::var_os("TACTUS_BLOCK_SIGNAL").is_some() {
            // SAFETY: this disposable process deliberately models an embedding
            // host that blocked SIGTERM before Tactus initialized supervision.
            unsafe {
                let mut blocked: libc::sigset_t = std::mem::zeroed();
                assert_eq!(libc::sigemptyset(&mut blocked), 0);
                assert_eq!(libc::sigaddset(&mut blocked, libc::SIGTERM), 0);
                assert_eq!(
                    libc::sigprocmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()),
                    0
                );
            }
        }
        let custom_handler = std::env::var_os("TACTUS_CUSTOM_SIGNAL_HANDLER").is_some();
        if custom_handler {
            CUSTOM_SIGNAL_SEEN.store(false, std::sync::atomic::Ordering::SeqCst);
            assert_ne!(
                unsafe {
                    libc::signal(
                        libc::SIGTERM,
                        record_custom_signal as *const () as libc::sighandler_t,
                    )
                },
                libc::SIG_ERR
            );
        }
        let custom_job_control = std::env::var_os("TACTUS_CUSTOM_JOB_CONTROL_HANDLER").is_some();
        if custom_job_control {
            CUSTOM_JOB_CONTROL_SEEN.store(false, std::sync::atomic::Ordering::SeqCst);
            CUSTOM_PARENT_PID.store(
                unsafe { libc::getpid() },
                std::sync::atomic::Ordering::SeqCst,
            );
            assert_ne!(
                unsafe {
                    libc::signal(
                        libc::SIGTSTP,
                        record_custom_job_control as *const () as libc::sighandler_t,
                    )
                },
                libc::SIG_ERR
            );
        }
        let progress_loop = std::env::var_os("TACTUS_SIGNAL_PROGRESS_LOOP").is_some();
        let mut command = if progress_loop {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command.args([
                "terminal_progress_worker_helper",
                "--ignored",
                "--nocapture",
            ]);
            command.env("TACTUS_SIGNAL_WORKER", "1");
            command
        } else {
            let script = "(sleep 1; printf leaked > \"$TACTUS_MARKER\") & worker=$!; \
             shell_pgid=$(ps -o pgid= -p $$ | tr -d ' '); \
             worker_pgid=$(ps -o pgid= -p $worker | tr -d ' '); \
             printf '%s %s %s %s' $$ $shell_pgid $worker $worker_pgid > \"$TACTUS_READY\"; \
             wait";
            shell(script)
        };
        command.env(
            "TACTUS_READY",
            std::env::var_os("TACTUS_READY").expect("ready path"),
        );
        command.env(
            "TACTUS_MARKER",
            std::env::var_os("TACTUS_MARKER").expect("marker path"),
        );
        if let Some(finish) = std::env::var_os("TACTUS_FINISH") {
            command.env("TACTUS_FINISH", finish);
        }
        let output =
            run_with_timeout(command, "", Duration::from_secs(30)).expect("signal helper command");
        if std::env::var_os("TACTUS_SIGNAL_HELPER_EXPECT_RETURN").is_some() {
            assert_eq!(output.code, Some(0), "supervised output: {output:?}");
            if custom_handler {
                assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
                assert!(
                    CUSTOM_SIGNAL_SEEN.load(std::sync::atomic::Ordering::SeqCst),
                    "Tactus replaced the embedding host's custom SIGTERM handler"
                );
            }
            if custom_job_control {
                assert!(
                    CUSTOM_JOB_CONTROL_SEEN.load(std::sync::atomic::Ordering::SeqCst),
                    "Tactus replaced the embedding host's custom SIGTSTP handler"
                );
            }
            return;
        }
        panic!("the helper should terminate with the forwarded signal");
    }

    #[cfg(unix)]
    static CUSTOM_SIGNAL_SEEN: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    #[cfg(unix)]
    static CUSTOM_JOB_CONTROL_SEEN: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    #[cfg(unix)]
    static CUSTOM_PARENT_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    #[cfg(unix)]
    extern "C" fn record_custom_signal(_: libc::c_int) {
        CUSTOM_SIGNAL_SEEN.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(unix)]
    extern "C" fn record_custom_job_control(_: libc::c_int) {
        let parent = CUSTOM_PARENT_PID.load(std::sync::atomic::Ordering::SeqCst);
        if unsafe { libc::getpid() } == parent {
            CUSTOM_JOB_CONTROL_SEEN.store(true, std::sync::atomic::Ordering::SeqCst);
        } else if parent > 0 {
            // A fork-copied host callback executing in the private guard is a
            // test failure: terminate the disposable parent immediately so the
            // outer test observes it rather than relying on private atomics.
            let _ = unsafe { libc::kill(parent, libc::SIGKILL) };
        }
    }

    #[cfg(unix)]
    struct SignalHelper {
        child: Child,
        scratch: std::path::PathBuf,
        marker: std::path::PathBuf,
        finish: std::path::PathBuf,
        diagnostic: std::path::PathBuf,
        supervised_pgid: Option<i32>,
        active: bool,
    }

    #[cfg(unix)]
    impl SignalHelper {
        fn pid(&self) -> i32 {
            i32::try_from(self.child.id()).expect("helper pid")
        }

        fn complete(&mut self) {
            self.active = false;
            let _ = std::fs::remove_dir_all(&self.scratch);
        }

        fn diagnostic(&self) -> String {
            std::fs::read_to_string(&self.diagnostic)
                .unwrap_or_else(|error| format!("<could not read helper diagnostic: {error}>"))
        }
    }

    #[cfg(unix)]
    impl Drop for SignalHelper {
        fn drop(&mut self) {
            if !self.active {
                return;
            }
            // A failed assertion must never strand either the helper's guard
            // group or its separately isolated agent group (the macOS runner
            // would otherwise wait forever for a suspended descendant).
            if let Some(pgid) = self.supervised_pgid {
                let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
            let _ = unsafe { libc::kill(-self.pid(), libc::SIGKILL) };
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.scratch);
        }
    }

    #[cfg(unix)]
    fn spawn_signal_helper(tag: &str, expect_return: bool, ignore_sighup: bool) -> SignalHelper {
        use std::os::unix::process::CommandExt;

        let scratch = std::env::temp_dir().join(format!(
            "tactus-proc-{tag}-{}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let ready = scratch.join("ready");
        let marker = scratch.join("leaked");
        let finish = scratch.join("finish");
        let diagnostic = scratch.join("helper.log");
        let diagnostic_stdout = std::fs::File::create(&diagnostic).expect("helper diagnostic");
        let diagnostic_stderr = diagnostic_stdout
            .try_clone()
            .expect("clone helper diagnostic");

        let mut helper = Command::new(std::env::current_exe().expect("test executable"));
        helper
            .args(["terminal_interrupt_helper", "--ignored", "--nocapture"])
            .env("TACTUS_SIGNAL_HELPER", "1")
            .env("TACTUS_READY", &ready)
            .env("TACTUS_MARKER", &marker)
            .env("TACTUS_FINISH", &finish)
            // Keep a broken child-group setup inside the disposable helper's
            // group. A regression must fail the test, never suspend the test
            // runner that is responsible for reporting and cleaning it up.
            .process_group(0)
            .stdout(Stdio::from(diagnostic_stdout))
            .stderr(Stdio::from(diagnostic_stderr));
        if expect_return {
            helper.env("TACTUS_SIGNAL_HELPER_EXPECT_RETURN", "1");
        }
        if tag.starts_with("job-control") || tag == "crash-lease" {
            helper.env("TACTUS_SIGNAL_PROGRESS_LOOP", "1");
        }
        if ignore_sighup {
            // SAFETY: `pre_exec` performs only the async-signal-safe `signal`
            // call. SIG_IGN is deliberately inherited across exec by POSIX.
            unsafe {
                helper.pre_exec(|| {
                    if libc::signal(libc::SIGHUP, libc::SIG_IGN) == libc::SIG_ERR {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        if matches!(tag, "custom-handler" | "job-control-custom") {
            helper.env("TACTUS_CUSTOM_SIGNAL_HANDLER", "1");
        }
        if tag == "custom-job-control" {
            helper.env("TACTUS_CUSTOM_JOB_CONTROL_HANDLER", "1");
        }
        if tag.contains("blocked") {
            helper.env("TACTUS_BLOCK_SIGNAL", "1");
            // Block before exec so every thread subsequently created by the
            // Rust test harness inherits the host policy. Blocking only in the
            // selected test thread would leave another harness thread able to
            // take the process-directed SIGTERM with its default disposition.
            unsafe {
                helper.pre_exec(|| {
                    let mut blocked: libc::sigset_t = std::mem::zeroed();
                    if libc::sigemptyset(&mut blocked) != 0
                        || libc::sigaddset(&mut blocked, libc::SIGTERM) != 0
                        || libc::sigprocmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()) != 0
                    {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        if tag == "crash-lease" {
            helper
                .env("TACTUS_CLEANUP_PUBLIC", scratch.join("run"))
                .env("TACTUS_TEST_CLEANUP_DELAY_MS", "700");
        }
        let child = helper.spawn().expect("spawn signal helper");
        let mut helper = SignalHelper {
            child,
            scratch,
            marker,
            finish,
            diagnostic,
            supervised_pgid: None,
            active: true,
        };

        let ready_deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < ready_deadline {
            if let Some(status) = helper.child.try_wait().expect("poll helper") {
                panic!("signal helper exited before its child was ready: {status}");
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "signal helper never spawned its child");
        let identities = std::fs::read_to_string(&ready).expect("signal helper identities");
        let fields = identities.split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "signal helper identities: {identities}");
        assert_eq!(
            fields[0], fields[1],
            "the supervised shell is not its process-group leader: {identities}"
        );
        assert_eq!(
            fields[1], fields[3],
            "the test descendant escaped the supervised group: {identities}"
        );
        helper.supervised_pgid = Some(fields[1].parse().expect("supervised process-group id"));
        helper
    }

    #[cfg(unix)]
    fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().expect("poll signal helper") {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    fn wait_for_stop(pid: i32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut status = 0;
            // SAFETY: callers pass an unreaped child pid; WNOHANG avoids an
            // unbounded wait and WUNTRACED reports the guard's SIGSTOP.
            let waited =
                unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
            assert!(waited >= 0, "waitpid: {}", std::io::Error::last_os_error());
            if waited == pid && libc::WIFSTOPPED(status) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[cfg(unix)]
    fn assert_termination_kills_the_isolated_tree(signal: libc::c_int, tag: &str) {
        let mut helper = spawn_signal_helper(tag, false, false);
        let pid = helper.pid();
        // SAFETY: the helper owns a dedicated process group. Terminal signals
        // target foreground groups, which also exercises the external guard.
        assert_eq!(unsafe { libc::kill(-pid, signal) }, 0);
        if wait_for_exit(&mut helper.child, Duration::from_secs(10)).is_none() {
            panic!("signalled supervisor did not terminate promptly");
        }

        thread::sleep(Duration::from_millis(1300));
        let leaked = helper.marker.exists();
        helper.complete();
        assert!(
            !leaked,
            "signal {signal} terminated Tactus but left its isolated agent tree alive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminal_interrupt_kills_the_isolated_tree() {
        assert_termination_kills_the_isolated_tree(libc::SIGINT, "interrupt");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_quit_kills_the_isolated_tree() {
        assert_termination_kills_the_isolated_tree(libc::SIGQUIT, "quit");
    }

    #[cfg(unix)]
    #[test]
    fn an_inherited_ignored_sighup_stays_ignored() {
        let mut helper = spawn_signal_helper("nohup", true, true);
        let pid = helper.pid();
        // SAFETY: the helper owns a dedicated process group.
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGHUP) }, 0);
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("ignored SIGHUP helper completes normally");
        let survived = helper.marker.exists();
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
        assert!(survived, "nohup-style SIGHUP unexpectedly killed the agent");
    }

    #[cfg(unix)]
    #[test]
    fn an_inherited_custom_signal_handler_is_preserved() {
        let mut helper = spawn_signal_helper("custom-handler", true, false);
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("custom-handler helper completes normally");
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn a_custom_job_control_callback_never_runs_in_the_guard() {
        let mut helper = spawn_signal_helper("custom-job-control", true, false);
        let pid = helper.pid();
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("custom job-control helper completes normally");
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn sigkill_of_tactus_job_still_reaps_the_isolated_agent_group() {
        let mut helper = spawn_signal_helper("job-control", true, false);
        let helper_pgid = helper.pid();
        let agent_pgid = helper.supervised_pgid.expect("supervised group");
        assert_eq!(unsafe { libc::kill(-helper_pgid, libc::SIGKILL) }, 0);
        wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("SIGKILLed helper exits promptly");

        // From here onward the test harness must not kill the agent on drop:
        // only the helper's external reaper is allowed to make progress stop.
        helper.active = false;
        thread::sleep(Duration::from_millis(1300));
        let before = std::fs::read_to_string(&helper.marker).ok();
        thread::sleep(Duration::from_millis(350));
        let after = std::fs::read_to_string(&helper.marker).ok();
        let stopped = before == after;

        // Clean up only after recording the result, so a regression cannot be
        // hidden while still avoiding a leaked worker after a failed test.
        let _ = unsafe { libc::kill(-agent_pgid, libc::SIGKILL) };
        helper.complete();
        assert!(
            stopped,
            "the isolated agent kept running after an uncatchable Tactus SIGKILL"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sigkill_keeps_resume_locked_out_until_agent_cleanup_finishes() {
        let mut helper = spawn_signal_helper("crash-lease", true, false);
        let public = helper.scratch.join("run");
        let helper_pgid = helper.pid();
        assert_eq!(unsafe { libc::kill(-helper_pgid, libc::SIGKILL) }, 0);
        wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("SIGKILLed lock holder exits promptly");

        let error = crate::rundir::RunLock::acquire(&public)
            .expect_err("the reaper-owned cleanup lease must block an overlapping resume");
        assert!(
            error.to_string().contains("already driving run"),
            "unexpected cleanup-lease refusal: {error}"
        );
        assert!(
            crate::rundir::is_running(&public),
            "liveness ignored the reaper-owned cleanup lease"
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        let recovered = loop {
            match crate::rundir::RunLock::acquire(&public) {
                Ok(lock) => break lock,
                Err(error) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                    drop(error);
                }
                Err(error) => panic!("cleanup lease never released: {error}"),
            }
        };
        drop(recovered);
        helper.complete();
    }

    #[cfg(unix)]
    #[test]
    fn terminal_suspend_and_continue_cover_the_isolated_tree() {
        let mut helper = spawn_signal_helper("job-control", true, false);
        let pid = helper.pid();
        // SAFETY: `pid` is the id of the helper's dedicated process group, so
        // this models terminal foreground-group job control without touching
        // the surrounding test runner.
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);

        assert!(
            wait_for_stop(pid, Duration::from_secs(10)),
            "Tactus did not enter a stopped job-control state"
        );

        let before =
            std::fs::read_to_string(&helper.marker).expect("progress before suspend interval");
        thread::sleep(Duration::from_millis(350));
        let after =
            std::fs::read_to_string(&helper.marker).expect("progress after suspend interval");
        assert_eq!(
            after, before,
            "the isolated agent kept making progress while Tactus was suspended"
        );

        std::fs::write(&helper.finish, "finish").expect("release supervised worker after continue");

        // SAFETY: SIGCONT resumes our helper; its installed handler forwards
        // the same transition to the isolated process group.
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("continued helper completes normally");
        let resumed = helper.marker.exists();
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
        assert!(resumed, "the isolated agent was not continued with Tactus");
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_terminal_signal_still_wakes_a_suspended_host() {
        let mut helper = spawn_signal_helper("job-control-blocked", true, false);
        let pid = helper.pid();
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        assert!(
            wait_for_stop(pid, Duration::from_secs(10)),
            "Tactus did not enter a stopped job-control state"
        );

        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTERM) }, 0);
        std::fs::write(&helper.finish, "finish").expect("release supervised worker");
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("guard with an unblocked mask wakes the suspended host");
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn a_custom_terminal_handler_still_wakes_a_suspended_host() {
        let mut helper = spawn_signal_helper("job-control-custom", true, false);
        let pid = helper.pid();
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        assert!(
            wait_for_stop(pid, Duration::from_secs(10)),
            "Tactus did not enter a stopped job-control state"
        );

        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTERM) }, 0);
        std::fs::write(&helper.finish, "finish").expect("release supervised worker");
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("guard relay wakes the custom-handler host");
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn an_ignored_sighup_does_not_wake_a_suspended_tree() {
        let mut helper = spawn_signal_helper("job-control-nohup", true, true);
        let pid = helper.pid();
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        assert!(
            wait_for_stop(pid, Duration::from_secs(10)),
            "Tactus did not enter a stopped job-control state"
        );
        let before =
            std::fs::read_to_string(&helper.marker).expect("progress before ignored SIGHUP");
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGHUP) }, 0);
        thread::sleep(Duration::from_millis(350));
        let after = std::fs::read_to_string(&helper.marker).expect("progress after ignored SIGHUP");
        assert_eq!(after, before, "ignored SIGHUP resumed the suspended agent");

        std::fs::write(&helper.finish, "finish").expect("release supervised worker");
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
        let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
            .expect("continued helper completes normally");
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn a_continue_racing_with_suspend_cannot_strand_the_tree() {
        let mut helper = spawn_signal_helper("job-control", true, false);
        let pid = helper.pid();
        // Deliver the transition back-to-back, before the monitor can promise
        // whether it has reached its final stop instruction.
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
        std::fs::write(&helper.finish, "finish").expect("release supervised worker");

        let status =
            wait_for_exit(&mut helper.child, Duration::from_secs(10)).unwrap_or_else(|| {
                panic!("a continue racing with suspend stranded Tactus or its agent tree");
            });
        let diagnostic = helper.diagnostic();
        helper.complete();
        assert!(status.success(), "helper status: {status}\n{diagnostic}");
    }

    #[cfg(unix)]
    #[test]
    fn termination_racing_with_suspend_still_kills_the_tree() {
        let mut helper = spawn_signal_helper("suspend-termination", false, false);
        let pid = helper.pid();
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
        // A terminal signal targets the foreground group. The guard remains
        // runnable and wakes a parent that SIGSTOP may already have committed.
        assert_eq!(unsafe { libc::kill(-pid, libc::SIGTERM) }, 0);
        if wait_for_exit(&mut helper.child, Duration::from_secs(10)).is_none() {
            panic!("termination racing with suspend did not terminate Tactus");
        }
        thread::sleep(Duration::from_millis(1300));
        let leaked = helper.marker.exists();
        helper.complete();
        assert!(!leaked, "the suspended agent tree survived termination");
    }

    #[test]
    fn missing_binary_is_a_spawn_error() {
        let cmd = Command::new("tactus-definitely-not-a-real-binary");
        let err = run_with_timeout(cmd, "", Duration::from_secs(1)).expect_err("must fail");
        assert!(err.to_string().contains("failed to spawn"));
    }
}
