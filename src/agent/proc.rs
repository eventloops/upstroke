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
//! suspension/continuation. It waits out any spawn-registration race, kills
//! every active process group before re-raising a terminating signal in
//! Tactus, and stops every active group whenever Tactus stops. The run lock can
//! therefore never be released -- or appear suspended -- while an isolated
//! agent is still running.

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
    static STATE: OnceLock<Result<Arc<Mutex<State>>, String>> = OnceLock::new();

    #[derive(Default)]
    struct State {
        /// Supervisors that entered before spawn but have not registered a pid.
        spawning: usize,
        /// Active isolated process-group ids.
        groups: Vec<i32>,
        /// Set by the monitor before it kills groups. No later spawn may begin.
        terminating: bool,
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
            let state = shared_state()?;
            let mut locked = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if locked.terminating || PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {
                return Err(TactusError::Agent {
                    message: "process launch interrupted by a termination signal".to_owned(),
                });
            }
            locked.spawning = locked.spawning.saturating_add(1);
            drop(locked);
            Ok(Self {
                state,
                phase: Phase::Spawning,
            })
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
        let state = Arc::new(Mutex::new(State::default()));
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
            if !is_ignored(signal)? {
                install_handler(signal)?;
            }
        }

        // Job-control proxying is useful only as a pair: if a launcher
        // explicitly ignored either half, preserve that policy rather than
        // stopping children we cannot reliably continue (or vice versa).
        if !is_ignored(libc::SIGTSTP)? && !is_ignored(libc::SIGCONT)? {
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
            libc::SIGCONT => CONTINUE_REQUESTED.store(true, Ordering::SeqCst),
            _ => {
                let _ = PENDING_TERMINATION.compare_exchange(
                    0,
                    signal,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
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
                // terminate Tactus with the original signal; `_exit` is a
                // defensive fallback if a platform returns from `raise`.
                unsafe {
                    libc::signal(terminating, libc::SIG_DFL);
                    libc::raise(terminating);
                    libc::_exit(128 + terminating);
                }
            }

            if SUSPEND_REQUESTED.swap(false, Ordering::SeqCst) {
                let Some(groups) = groups_when_registered(&state, false) else {
                    SUSPEND_REQUESTED.store(true, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                // SIGSTOP cannot be caught or ignored, so a vendor process
                // cannot keep spending while its visibly foreground Tactus
                // parent is suspended. SIGCONT below releases the same groups.
                signal_groups(&groups, libc::SIGSTOP);

                // A terminating signal wins over suspension. Do not stop the
                // parent after its monitor has already been asked to tear down.
                if PENDING_TERMINATION.load(Ordering::SeqCst) != 0 {
                    continue;
                }
                if CONTINUE_REQUESTED.swap(false, Ordering::SeqCst) {
                    signal_groups(&groups, libc::SIGCONT);
                    continue;
                }

                // SAFETY: SIGSTOP suspends the whole process without changing
                // any installed disposition. Delivery of SIGCONT resumes it;
                // our SIGCONT handler then asks this monitor to resume every
                // isolated child group as well.
                let _ = unsafe { libc::kill(libc::getpid(), libc::SIGSTOP) };
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

    fn signal_groups(groups: &[i32], signal: libc::c_int) {
        for pgid in groups {
            // SAFETY: every registered child was created with
            // `process_group(0)`, so its pid is its private group id. A
            // negative id targets that group and never Tactus's group.
            let _ = unsafe { libc::kill(-*pgid, signal) };
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
        let mut command = shell(
            "printf ready > \"$TACTUS_READY\"; \
             (sleep 1; printf leaked > \"$TACTUS_MARKER\") & wait",
        );
        command.env(
            "TACTUS_READY",
            std::env::var_os("TACTUS_READY").expect("ready path"),
        );
        command.env(
            "TACTUS_MARKER",
            std::env::var_os("TACTUS_MARKER").expect("marker path"),
        );
        let output =
            run_with_timeout(command, "", Duration::from_secs(30)).expect("signal helper command");
        if std::env::var_os("TACTUS_SIGNAL_HELPER_EXPECT_RETURN").is_some() {
            assert_eq!(output.code, Some(0));
            return;
        }
        panic!("the helper should terminate with the forwarded signal");
    }

    #[cfg(unix)]
    fn spawn_signal_helper(
        tag: &str,
        expect_return: bool,
        ignore_sighup: bool,
    ) -> (Child, std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::process::CommandExt;

        let scratch = std::env::temp_dir().join(format!(
            "tactus-proc-{tag}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch dir");
        let ready = scratch.join("ready");
        let marker = scratch.join("leaked");

        let mut helper = Command::new(std::env::current_exe().expect("test executable"));
        helper
            .args(["terminal_interrupt_helper", "--ignored", "--nocapture"])
            .env("TACTUS_SIGNAL_HELPER", "1")
            .env("TACTUS_READY", &ready)
            .env("TACTUS_MARKER", &marker)
            // Keep a broken child-group setup inside the disposable helper's
            // group. A regression must fail the test, never suspend the test
            // runner that is responsible for reporting and cleaning it up.
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if expect_return {
            helper.env("TACTUS_SIGNAL_HELPER_EXPECT_RETURN", "1");
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
        let mut helper = helper.spawn().expect("spawn signal helper");

        let ready_deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < ready_deadline {
            if let Some(status) = helper.try_wait().expect("poll helper") {
                panic!("signal helper exited before its child was ready: {status}");
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "signal helper never spawned its child");
        (helper, scratch, marker)
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
    fn assert_termination_kills_the_isolated_tree(signal: libc::c_int, tag: &str) {
        let (mut helper, scratch, marker) = spawn_signal_helper(tag, false, false);
        let pid = i32::try_from(helper.id()).expect("helper pid");
        // SAFETY: `pid` names only the dedicated helper subprocess.
        assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
        if wait_for_exit(&mut helper, Duration::from_secs(10)).is_none() {
            let _ = helper.kill();
            let _ = helper.wait();
            panic!("signalled supervisor did not terminate promptly");
        }

        thread::sleep(Duration::from_millis(1300));
        let leaked = marker.exists();
        let _ = std::fs::remove_dir_all(&scratch);
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
        let (mut helper, scratch, marker) = spawn_signal_helper("nohup", true, true);
        let pid = i32::try_from(helper.id()).expect("helper pid");
        // SAFETY: `pid` names only the dedicated helper subprocess.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGHUP) }, 0);
        let status = wait_for_exit(&mut helper, Duration::from_secs(10))
            .expect("ignored SIGHUP helper completes normally");
        let survived = marker.exists();
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(status.success(), "helper status: {status}");
        assert!(survived, "nohup-style SIGHUP unexpectedly killed the agent");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_suspend_and_continue_cover_the_isolated_tree() {
        let (mut helper, scratch, marker) = spawn_signal_helper("job-control", true, false);
        let pid = i32::try_from(helper.id()).expect("helper pid");
        // SAFETY: `pid` names only the dedicated helper subprocess.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTSTP) }, 0);

        let stopped_deadline = Instant::now() + Duration::from_secs(10);
        let mut stopped = false;
        while Instant::now() < stopped_deadline {
            let mut status = 0;
            // SAFETY: the helper is our unreaped child; WNOHANG avoids an
            // unbounded test wait and WUNTRACED reports the monitor's SIGSTOP.
            let waited =
                unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
            assert!(waited >= 0, "waitpid: {}", std::io::Error::last_os_error());
            if waited == pid && libc::WIFSTOPPED(status) {
                stopped = true;
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(stopped, "Tactus did not enter a stopped job-control state");

        thread::sleep(Duration::from_millis(1300));
        assert!(
            !marker.exists(),
            "the isolated agent kept running while Tactus was suspended"
        );

        // SAFETY: SIGCONT resumes our helper; its installed handler forwards
        // the same transition to the isolated process group.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
        let status = wait_for_exit(&mut helper, Duration::from_secs(10))
            .expect("continued helper completes normally");
        let resumed = marker.exists();
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(status.success(), "helper status: {status}");
        assert!(resumed, "the isolated agent was not continued with Tactus");
    }

    #[test]
    fn missing_binary_is_a_spawn_error() {
        let cmd = Command::new("tactus-definitely-not-a-real-binary");
        let err = run_with_timeout(cmd, "", Duration::from_secs(1)).expect_err("must fail");
        assert!(err.to_string().contains("failed to spawn"));
    }
}
