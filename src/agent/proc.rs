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
//! process-wide signal monitor below preserves inherited ignored signals,
//! coordinates SIGINT/SIGTERM/SIGHUP/SIGQUIT termination, and proxies terminal
//! suspension/continuation. It waits out any spawn-registration race, blocks
//! launches across a suspension transition, and uses a descriptor-scrubbed
//! guard process to close the last signal-to-stop race. It kills every active
//! process group before re-raising a terminating signal in Tactus, and stops
//! every active group whenever Tactus stops. The run lock can therefore never
//! be released -- or appear suspended -- while an isolated agent is running.

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

    // A supervised Unix invocation owns a fresh process group. Vendor CLIs
    // routinely launch native children; killing only the shell/Node parent
    // leaves those children consuming quota and holding our pipe handles.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    // Enter before `spawn`: if an interrupt arrives in the narrow interval
    // between creating the child and learning its pid, the signal monitor
    // waits for this registration rather than terminating Tactus first and
    // orphaning the new process group.
    #[cfg(unix)]
    let mut termination = termination::Supervisor::begin()?;

    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| TactusError::Agent {
        message: format!(
            "failed to spawn `{}`: {e}",
            command.get_program().to_string_lossy()
        ),
    })?;
    #[cfg(unix)]
    if let Err(error) = termination.register(child.id()) {
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
    static GUARD_SIGNAL: AtomicI32 = AtomicI32::new(0);
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

    #[derive(Clone, Copy)]
    struct SignalPolicy {
        termination_mask: u8,
        job_control: bool,
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
    }

    pub(super) struct Supervisor {
        state: Arc<Mutex<State>>,
        phase: Phase,
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
                if !locked.suspending {
                    locked.spawning = locked.spawning.saturating_add(1);
                    drop(locked);
                    return Ok(Self {
                        state,
                        phase: Phase::Spawning,
                    });
                }
                drop(locked);
                thread::sleep(Duration::from_millis(1));
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
    }

    impl Drop for Supervisor {
        fn drop(&mut self) {
            let mut locked = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match self.phase {
                Phase::Spawning => locked.spawning = locked.spawning.saturating_sub(1),
                Phase::Group(pgid) => {
                    if let Some(index) = locked
                        .groups
                        .iter()
                        .position(|candidate| *candidate == pgid)
                    {
                        locked.groups.swap_remove(index);
                    }
                }
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
            job_control: false,
        };
        for (signal, bit) in [
            (libc::SIGINT, HANDLE_SIGINT),
            (libc::SIGTERM, HANDLE_SIGTERM),
            (libc::SIGHUP, HANDLE_SIGHUP),
            (libc::SIGQUIT, HANDLE_SIGQUIT),
        ] {
            if !is_ignored(signal)? {
                policy.termination_mask |= bit;
            }
        }
        policy.job_control = !is_ignored(libc::SIGTSTP)? && !is_ignored(libc::SIGCONT)?;

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
            // POSIX preserves SIG_IGN across exec. `nohup` relies on that for
            // SIGHUP, and replacing it would make Tactus less durable merely
            // because it supervised a command. Explicitly ignored terminating
            // signals retain the disposition the launcher chose.
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

    fn is_ignored(signal: libc::c_int) -> Result<bool, String> {
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
            Ok(previous.sa_sigaction == libc::SIG_IGN)
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
        GUARD_SIGNAL.store(signal, Ordering::SeqCst);
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

        // SAFETY: the child enters `guard_loop` immediately, which uses only
        // libc syscalls and lock-free atomics after fork. It closes every
        // inherited descriptor except its two pipes before doing any work.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            for fd in [command[0], command[1], ack[0], ack[1]] {
                close_fd(fd);
            }
            return Err(format!(
                "starting Unix job-control guard: {}",
                std::io::Error::last_os_error()
            ));
        }
        if pid == 0 {
            close_fd(command[1]);
            close_fd(ack[0]);
            close_inherited_fds(command[0], ack[1], open_max);
            guard_loop(unsafe { libc::getppid() }, command[0], ack[1], policy);
        }

        close_fd(ack[1]);
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
        policy: SignalPolicy,
    ) -> ! {
        install_guard_dispositions(policy);
        if !write_byte(ack_fd, GUARD_READY) {
            unsafe { libc::_exit(1) };
        }
        let mut armed = false;
        let mut stopping = false;
        let mut wake = false;
        let mut buffer = [0_u8; 64];
        loop {
            let mut poll_fd = libc::pollfd {
                fd: command_fd,
                events: libc::POLLIN | libc::POLLHUP,
                revents: 0,
            };
            // SAFETY: `poll_fd` is valid for one entry. A foreground-group
            // signal interrupts the poll and is observed through GUARD_SIGNAL;
            // parent-PID signals arrive over the ordered command pipe.
            let polled = unsafe { libc::poll(&mut poll_fd, 1, -1) };
            if polled < 0 && !last_errno_is_interrupted() {
                unsafe { libc::_exit(1) };
            }
            if polled > 0 && poll_fd.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
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
                            // by its ordered pipe record or GUARD_SIGNAL.
                            wake = false;
                            stopping = false;
                            GUARD_SIGNAL.store(0, Ordering::SeqCst);
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
                            GUARD_SIGNAL.store(0, Ordering::SeqCst);
                        }
                        _ => wake = true,
                    }
                }
            }
            if GUARD_SIGNAL.swap(0, Ordering::SeqCst) != 0 {
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

    fn install_guard_dispositions(policy: SignalPolicy) {
        // The guard stays in the foreground process group but cannot join the
        // stop: it ignores SIGTSTP and records every transition that must wake
        // a parent already stopped by the guard. SIGSTOP itself targets only
        // the parent pid.
        unsafe {
            if policy.job_control {
                libc::signal(libc::SIGTSTP, libc::SIG_IGN);
                libc::signal(
                    libc::SIGCONT,
                    record_guard_signal as *const () as libc::sighandler_t,
                );
            }
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                if policy.handles_termination(signal) {
                    libc::signal(
                        signal,
                        record_guard_signal as *const () as libc::sighandler_t,
                    );
                }
            }
        }
    }

    fn close_inherited_fds(keep_a: libc::c_int, keep_b: libc::c_int, open_max: libc::c_int) {
        // The fork must not retain the run lock, event file, pipes, or secrets.
        // Closing descriptors is async-signal-safe and the child never returns
        // to Rust runtime code after this point.
        #[cfg(target_os = "linux")]
        if close_linux_ranges_except(keep_a, keep_b) {
            return;
        }
        for fd in 0..open_max {
            if fd != keep_a && fd != keep_b {
                close_fd(fd);
            }
        }
    }

    fn last_errno_is_interrupted() -> bool {
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location() == libc::EINTR
        }
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() == libc::EINTR
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        }
    }

    #[cfg(target_os = "linux")]
    fn close_linux_ranges_except(keep_a: libc::c_int, keep_b: libc::c_int) -> bool {
        let low = keep_a.min(keep_b) as libc::c_uint;
        let high = keep_a.max(keep_b) as libc::c_uint;
        let ranges = [
            (0, low.saturating_sub(1), low > 0),
            (
                low.saturating_add(1),
                high.saturating_sub(1),
                high > low + 1,
            ),
            (
                high.saturating_add(1),
                libc::c_uint::MAX,
                high < libc::c_uint::MAX,
            ),
        ];
        for (first, last, present) in ranges {
            if !present {
                continue;
            }
            // SAFETY: close_range is invoked through the raw syscall so older
            // kernels can return ENOSYS without imposing a newer glibc symbol.
            if unsafe { libc::syscall(libc::SYS_close_range, first, last, 0_u32) } != 0 {
                return false;
            }
        }
        true
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

    fn groups_are_quiescent(groups: &[i32]) -> bool {
        if groups.is_empty() {
            return true;
        }
        // `/bin/ps` is a fixed system interface on the supported Unix targets;
        // no repository-controlled PATH entry can substitute for it. Unlike
        // waitid, this observes every descendant in the process group, not
        // only Tactus's direct child.
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
            if !matches!(*state, b'T' | b'Z') {
                return false;
            }
        }
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
            assert_eq!(output.code, Some(0));
            return;
        }
        panic!("the helper should terminate with the forwarded signal");
    }

    #[cfg(unix)]
    struct SignalHelper {
        child: Child,
        scratch: std::path::PathBuf,
        marker: std::path::PathBuf,
        finish: std::path::PathBuf,
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
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if expect_return {
            helper.env("TACTUS_SIGNAL_HELPER_EXPECT_RETURN", "1");
        }
        if tag.starts_with("job-control") {
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
        let child = helper.spawn().expect("spawn signal helper");
        let mut helper = SignalHelper {
            child,
            scratch,
            marker,
            finish,
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
        helper.complete();
        assert!(status.success(), "helper status: {status}");
        assert!(survived, "nohup-style SIGHUP unexpectedly killed the agent");
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
        helper.complete();
        assert!(status.success(), "helper status: {status}");
        assert!(resumed, "the isolated agent was not continued with Tactus");
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
        helper.complete();
        assert!(status.success(), "helper status: {status}");
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
        helper.complete();
        assert!(status.success(), "helper status: {status}");
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
