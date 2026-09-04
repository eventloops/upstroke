//! Extended notes: `docs/internals/agent/proc/tests.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::topology::effects::Injection;

#[cfg(windows)]
use super::ambient::join_ambient_job_with;
use super::test_support::readiness;
use super::test_support::run_with_timeout;
use super::*;

#[test]
fn a_memoised_establishment_failure_reaches_every_later_caller() {
    assert_eq!(memoised_outcome::<()>(&Ok(())), Ok(()));

    for message in [
        "it could not be created (Access is denied. (os error 5))",
        "it could not be configured (os error 87)",
        "AssignProcessToJobObject refused",
    ] {
        assert_eq!(
            memoised_outcome::<()>(&Err(message.to_owned())),
            Err(message.to_owned()),
            "a remembered failure must come back as that failure"
        );
    }

    let memo: Result<(), String> = Err("it could not be created".to_owned());
    assert_eq!(memoised_outcome(&memo), memoised_outcome(&memo));
    assert!(memoised_outcome(&memo).is_err());
}

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
    let out =
        run_with_timeout(shell("echo hello"), "", Duration::from_secs(30)).expect("spawn shell");
    assert_eq!(out.code, Some(0));
    assert!(out.stdout.contains("hello"));
    assert!(!out.timed_out);
    assert!(!out.output_limited);
}

#[test]
fn invalid_process_site_pairs_fail_before_spawn_in_release_code() {
    for (spawn, terminate) in [
        (ProcessSite::Spawn, ProcessSite::Spawn),
        (ProcessSite::Terminate, ProcessSite::Terminate),
        (ProcessSite::Terminate, ProcessSite::Spawn),
    ] {
        let error = run_with_timeout_at(
            spawn,
            terminate,
            Command::new("upstroke-site-validation-must-not-spawn"),
            b"",
            Duration::from_secs(1),
            &mut NoHooks,
        )
        .expect_err("an invalid Process-site pair must fail closed");
        let message = error.to_string();
        assert!(message.contains("process funnel requires"), "{message}");
        assert!(!message.contains("failed to spawn"), "{message}");
    }
}

#[test]
#[ignore = "subprocess helper"]
fn excessive_output_helper() {
    let Some(budget) = std::env::var_os("UPSTROKE_EXCESSIVE_OUTPUT_HELPER") else {
        return;
    };
    let budget: usize = budget
        .to_string_lossy()
        .parse()
        .expect("the helper's byte budget");
    let chunk = [b'x'; 4096];
    let on_stderr = std::env::var_os("UPSTROKE_EXCESSIVE_OUTPUT_STREAM")
        .is_some_and(|stream| stream == "stderr");
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let mut written = 0_usize;
    while written < budget {
        let sink: &mut dyn Write = if on_stderr { &mut stderr } else { &mut stdout };
        sink.write_all(&chunk)
            .expect("write deterministic excessive output");
        written += chunk.len();
    }
    thread::sleep(Duration::from_secs(15));
}

const EXCESSIVE_OUTPUT_BUDGET: usize = 64 * 1024 * 1024;

#[test]
fn excessive_output_is_bounded_and_terminates_the_tree() {
    const TEST_LIMIT: usize = 64 * 1024;
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["excessive_output_helper", "--ignored", "--nocapture"])
        .env(
            "UPSTROKE_EXCESSIVE_OUTPUT_HELPER",
            EXCESSIVE_OUTPUT_BUDGET.to_string(),
        );

    let started = Instant::now();
    let out = run_with_timeout_and_limit(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        command,
        b"",
        Duration::from_secs(30),
        TEST_LIMIT,
        &mut NoHooks,
    )
    .expect("supervise noisy child");
    assert!(out.output_limited, "supervised output: {out:?}");
    assert!(!out.timed_out);
    assert!(out.code.is_none());
    assert!(out.stdout.len() <= TEST_LIMIT);
    assert!(out.stderr.len() <= TEST_LIMIT);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "output-limited child was not terminated promptly: {:?}",
        started.elapsed()
    );
}

#[test]
fn the_output_allowance_bounds_stderr_as_well_as_stdout() {
    const TEST_LIMIT: usize = 64 * 1024;

    let small = run_with_timeout_and_limit(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        shell("echo problem 1>&2"),
        b"",
        Duration::from_secs(60),
        TEST_LIMIT,
        &mut NoHooks,
    )
    .expect("supervise a modest stderr writer");
    assert!(
        !small.output_limited,
        "a small writer was limited: {small:?}"
    );
    assert!(
        small.stderr.contains("problem"),
        "the control fixture wrote nothing to stderr: {small:?}"
    );

    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["excessive_output_helper", "--ignored", "--nocapture"])
        .env(
            "UPSTROKE_EXCESSIVE_OUTPUT_HELPER",
            EXCESSIVE_OUTPUT_BUDGET.to_string(),
        )
        .env("UPSTROKE_EXCESSIVE_OUTPUT_STREAM", "stderr");
    let started = Instant::now();
    let out = run_with_timeout_and_limit(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        command,
        b"",
        Duration::from_secs(60),
        TEST_LIMIT,
        &mut NoHooks,
    )
    .expect("supervise a noisy stderr child");
    assert!(
        out.output_limited,
        "a stderr-only producer was never bounded: {out:?}"
    );
    assert!(
        out.code.is_none(),
        "the stderr-limited child exited on its own terms rather than \
         being terminated: {out:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the stderr-limited tree was not terminated promptly: {:?}",
        started.elapsed()
    );
    assert!(!out.timed_out, "{out:?}");
    assert!(out.stderr.len() <= TEST_LIMIT, "{}", out.stderr.len());
    assert!(
        !out.stdout.contains("xxxx"),
        "the stderr fixture wrote its payload to stdout, so the bound this \
         test observed was stdout's after all"
    );
}

#[test]
#[ignore = "subprocess helper"]
fn stdin_hex_helper() {
    if std::env::var_os("UPSTROKE_STDIN_HEX").is_none() {
        return;
    }
    let mut received = Vec::new();
    std::io::stdin()
        .read_to_end(&mut received)
        .expect("read stdin");
    let mut hex = String::new();
    for byte in &received {
        hex.push_str(&format!("{byte:02x}"));
    }
    print!("<{hex}>");
    let _ = std::io::stdout().flush();
}

#[test]
fn stdin_reaches_the_child_byte_for_byte() {
    let payload: Vec<u8> = vec![0x00, 0x80, 0xff, 0x0a, 0x41];
    assert_ne!(
        String::from_utf8_lossy(&payload).as_bytes(),
        payload.as_slice(),
        "the fixture must be bytes a lossy conversion would change, or the \
         mutation this test exists for is invisible to it too"
    );
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["stdin_hex_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_STDIN_HEX", "1");
    let out = run_with_timeout_at(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        command,
        &payload,
        Duration::from_secs(60),
        &mut NoHooks,
    )
    .expect("supervise the stdin helper");
    let expected: String = payload.iter().map(|byte| format!("{byte:02x}")).collect();
    assert!(
        out.stdout.contains(&format!("<{expected}>")),
        "the child did not receive the bytes this test wrote: {} (wanted {expected})",
        out.stdout
    );
}

#[test]
#[ignore = "subprocess helper"]
fn timeout_transcript_helper() {
    if std::env::var_os("UPSTROKE_TIMEOUT_TRANSCRIPT").is_none() {
        return;
    }
    print!("OUT-BEFORE-TIMEOUT");
    let _ = std::io::stdout().flush();
    eprint!("ERR-BEFORE-TIMEOUT");
    let _ = std::io::stderr().flush();
    thread::sleep(Duration::from_secs(60));
}

#[test]
fn a_timed_out_child_keeps_the_transcript_it_had_already_written() {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["timeout_transcript_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_TIMEOUT_TRANSCRIPT", "1");
    let out = run_with_timeout(command, "", Duration::from_secs(3))
        .expect("supervise the transcript helper");
    assert!(out.timed_out, "{out:?}");
    assert!(
        out.stdout.contains("OUT-BEFORE-TIMEOUT"),
        "the timed-out child's stdout was discarded: {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("ERR-BEFORE-TIMEOUT"),
        "the timed-out child's stderr was discarded: {:?}",
        out.stderr
    );
}

#[cfg(unix)]
#[test]
#[allow(clippy::zombie_processes)]
fn a_child_registered_pre_exec_is_settled_when_the_parent_never_registers_it() {
    use std::os::unix::process::ExitStatusExt;

    let supervisor =
        termination::Supervisor::begin(ProcessSite::Terminate).expect("start a private reaper");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "sleep 60"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    supervisor.prepare(&mut command);
    let mut child = command
        .spawn()
        .expect("spawn a child that outlives the parent's window");
    let pid = child.id();
    assert!(
        child_leads_its_own_group(pid),
        "the pre-exec closure did not run at all, so this witnesses nothing"
    );

    drop(supervisor);

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut settled = None;
    while Instant::now() < deadline {
        match child.try_wait().expect("poll the child") {
            Some(status) => {
                settled = Some(status);
                break;
            }
            None => thread::sleep(Duration::from_millis(20)),
        }
    }
    let reaped_before_the_deadline = settled.is_some();
    if settled.is_none() {
        let _ = child.kill();
        settled = child.wait().ok();
    }
    assert!(
        reaped_before_the_deadline,
        "the child's group outlived a cancelled reaper: nothing registered it, so the \
         registration is not happening in the child before exec"
    );
    let status = settled.expect("the child could not be waited on at all");
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "the group was settled by something other than the reaper: {status:?}"
    );
}

#[test]
fn nonzero_exit_is_reported_not_an_error() {
    let out = run_with_timeout(shell("exit 3"), "", Duration::from_secs(30)).expect("spawn shell");
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
    let script = if cfg!(windows) {
        "ping -n 30 127.0.0.1 > NUL"
    } else {
        "sleep 30"
    };
    let started = Instant::now();
    let out = run_with_timeout(shell(script), "", Duration::from_millis(300)).expect("spawn shell");
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
        "upstroke-proc-tree-{}-{}.marker",
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    ));
    let _ = std::fs::remove_file(&marker);

    let mut command = shell("(sleep 1; printf leaked > \"$UPSTROKE_MARKER\") & wait");
    command.env("UPSTROKE_MARKER", &marker);
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
fn every_pipe_writer_is_gone(fd: libc::c_int) -> bool {
    let mut buffer = [0_u8; 256];
    loop {
        // SAFETY: `fd` is a live non-blocking read end owned by this test
        // and `buffer` is a writable buffer of the length passed.
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        match read {
            0 => return true,
            1.. => (),
            _ => return false,
        }
    }
}

#[cfg(unix)]
#[test]
fn kill_tree_settles_the_whole_unix_group_before_it_returns() {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let scratch = std::env::temp_dir().join(format!(
        "upstroke-kill-tree-{}-{}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let ready = scratch.join("ready");
    let mut command = shell(
        "sh -c 'printf \"ready\\n\" > \"$UPSTROKE_READY.publishing\"; \
         mv \"$UPSTROKE_READY.publishing\" \"$UPSTROKE_READY\"; sleep 60' & sleep 60",
    );
    command
        .env("UPSTROKE_READY", &ready)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // SAFETY: the closure calls one async-signal-safe syscall. The group is
    // what `kill_tree` targets, so the fixture must have one of its own.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut tree = ProcessTree::spawn(&mut command, &mut NoHooks).expect("spawn a group leader");
    let pgid = i32::try_from(tree.child.id()).expect("pid fits");
    let stderr = tree.child.stderr.take().expect("piped stderr");
    let fd = stderr.as_raw_fd();
    // SAFETY: `fd` is owned by `stderr`, which outlives this call.
    unsafe {
        libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK);
    }
    let published = readiness::await_signal(&ready, &mut tree.child, Duration::from_secs(10))
        .or_fail("the grandchild never started");
    assert_eq!(
        published,
        ["ready"],
        "the marker became visible with its content, not before it"
    );
    assert!(
        !every_pipe_writer_is_gone(fd),
        "the fixture holds no pipe, so this test would pass vacuously"
    );

    kill_tree(ProcessSite::Terminate, &mut tree).expect("settle the group");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut settled = every_pipe_writer_is_gone(fd);
    while !settled && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
        settled = every_pipe_writer_is_gone(fd);
    }
    // SAFETY: a negative pid names the group; this is cleanup for the
    // failing case and a no-op for the passing one.
    unsafe {
        let _ = libc::kill(-pgid, libc::SIGKILL);
    }
    drop(stderr);
    drop(tree);
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        settled,
        "kill_tree returned while a member of the child's process group was still \
         running and still holding its pipes: only the direct child was killed"
    );
}

#[cfg(unix)]
#[test]
fn a_successful_direct_exit_settles_its_group_before_the_transcript_is_collected() {
    let out = run_with_timeout(
        shell("sh -c 'sleep 1; printf ESCAPED' & exit 0"),
        "",
        Duration::from_secs(30),
    )
    .expect("spawn shell");
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(
        !out.stdout.contains("ESCAPED"),
        "a grandchild outlived the successful direct child and wrote into its \
         transcript: {}",
        out.stdout
    );
}

#[cfg(unix)]
#[test]
fn every_unix_containment_point_is_measured_against_its_own_operation() {
    use std::os::unix::ffi::OsStrExt;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq, Eq)]
    struct Row {
        point: SubEffectPoint,
        child_known: bool,
        leads_own_group: Option<bool>,
        registered: usize,
        cleanup_hold_taken: bool,
    }

    #[derive(Clone)]
    struct Observer {
        pid: Arc<Mutex<Option<u32>>>,
        rows: Arc<Mutex<Vec<Row>>>,
        cleanup: std::ffi::CString,
    }

    impl Observer {
        fn hold_taken(&self) -> bool {
            // SAFETY: a null-terminated path this test built; a failure
            // returns a negative descriptor.
            let fd = unsafe { libc::open(self.cleanup.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            if fd < 0 {
                return false;
            }
            // SAFETY: `fd` is live and owned here until the close below.
            unsafe {
                let free = libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0;
                if free {
                    let _ = libc::flock(fd, libc::LOCK_UN);
                }
                let _ = libc::close(fd);
                !free
            }
        }
    }

    impl SpawnHooks for Observer {
        fn child_created(&mut self, pid: u32) {
            *self.pid.lock().expect("pid") = Some(pid);
        }

        fn point(&mut self, point: SubEffectPoint) -> Injection {
            let pid = *self.pid.lock().expect("pid");
            let pgid = pid.and_then(|pid| i32::try_from(pid).ok());
            let row = Row {
                point,
                child_known: pid.is_some(),
                leads_own_group: pid.map(child_leads_its_own_group),
                registered: pgid.map_or(0, |pgid| {
                    termination::registered_groups()
                        .iter()
                        .filter(|group| **group == pgid)
                        .count()
                }),
                cleanup_hold_taken: self.hold_taken(),
            };
            self.rows.lock().expect("rows").push(row);
            Injection::Proceed
        }
    }

    let public = std::env::temp_dir().join(format!(
        "upstroke-r28-points-{}-{}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    std::fs::create_dir_all(&public).expect("run directory");
    let lock = crate::rundir::RunLock::acquire(&public).expect("take the run lock");
    let scope = lock.enter_cleanup_scope();
    let paths = crate::rundir::active_cleanup_lease_paths();
    assert_eq!(
        paths.len(),
        1,
        "exactly one cleanup lease is active: {paths:?}"
    );
    let cleanup =
        std::ffi::CString::new(paths[0].as_os_str().as_bytes()).expect("path without a null");

    let observer = Observer {
        pid: Arc::new(Mutex::new(None)),
        rows: Arc::new(Mutex::new(Vec::new())),
        cleanup,
    };
    assert!(
        !observer.hold_taken(),
        "R28 is already held before the reaper exists, so this test proves nothing"
    );
    let mut hooks = observer.clone();
    let output = run_with_timeout_at(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        shell("exit 0"),
        b"",
        Duration::from_secs(30),
        &mut hooks,
    )
    .expect("run through the funnel");
    assert_eq!(output.code, Some(0), "{output:?}");
    drop(scope);
    drop(lock);
    let _ = std::fs::remove_dir_all(&public);

    let observed = observer.rows.lock().expect("rows");
    let expected = vec![
        Row {
            point: SubEffectPoint::ReaperStarted,
            child_known: false,
            leads_own_group: None,
            registered: 0,
            cleanup_hold_taken: true,
        },
        Row {
            point: SubEffectPoint::PreExecPgidAndRegister,
            child_known: true,
            leads_own_group: Some(true),
            registered: 0,
            cleanup_hold_taken: true,
        },
        Row {
            point: SubEffectPoint::Exec,
            child_known: true,
            leads_own_group: Some(true),
            registered: 0,
            cleanup_hold_taken: true,
        },
        Row {
            point: SubEffectPoint::Registered,
            child_known: true,
            leads_own_group: Some(true),
            registered: 1,
            cleanup_hold_taken: true,
        },
    ];
    assert_eq!(
        *observed, expected,
        "a containment point no longer sits at the coordinate it names"
    );
}

#[cfg(unix)]
#[test]
fn every_unix_containment_point_is_gated_on_unix_and_not_on_one_unix() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent/proc.rs"),
    )
    .expect("read the funnel's own source");
    let lines: Vec<&str> = source.lines().collect();

    let mut gates: Vec<(&str, &str)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        const CALL: &str = "hooks.point(SubEffectPoint::";
        let Some(at) = line.find(CALL) else {
            continue;
        };
        let Some(point) = line[at + CALL.len()..].split(')').next() else {
            continue;
        };
        let gate = lines[..index]
            .iter()
            .rev()
            .find(|earlier| earlier.trim_start().starts_with("#[cfg("))
            .map(|earlier| earlier.trim())
            .unwrap_or("<none>");
        gates.push((point, gate));
    }

    let expected = vec![
        ("ReaperStarted", "#[cfg(unix)]"),
        ("PreExecPgidAndRegister", "#[cfg(unix)]"),
        ("Exec", "#[cfg(unix)]"),
        ("Registered", "#[cfg(unix)]"),
    ];
    let unix_gates: Vec<(&str, &str)> = gates
        .into_iter()
        .filter(|(point, _)| {
            matches!(
                *point,
                "ReaperStarted" | "PreExecPgidAndRegister" | "Exec" | "Registered"
            )
        })
        .collect();
    assert_eq!(
        unix_gates, expected,
        "a Unix containment point is compiled behind something other than \
         `cfg(unix)`; `os_matrix` says Linux **and macOS**"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess helper"]
#[allow(clippy::zombie_processes)]
fn unix_reaper_reparent_helper() {
    if std::env::var_os("UPSTROKE_UNIX_REPARENT").is_none() {
        return;
    }
    let ready = std::path::PathBuf::from(std::env::var_os("UPSTROKE_READY").expect("ready path"));
    let agent = std::path::PathBuf::from(std::env::var_os("UPSTROKE_AGENT").expect("agent path"));
    let mut supervisor =
        termination::Supervisor::begin(ProcessSite::Terminate).expect("start a private reaper");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "sleep 120"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    supervisor.prepare(&mut command);
    let child = command.spawn().expect("spawn an agent in its own group");
    supervisor
        .register(child.id())
        .expect("register the agent group");
    std::fs::write(&agent, child.id().to_string()).expect("record the agent pid");
    // SAFETY: the forked child calls only `sleep` and `_exit`, both
    // async-signal-safe, and never returns to the Rust runtime.
    let forked = unsafe { libc::fork() };
    if forked == 0 {
        unsafe {
            libc::sleep(120);
            libc::_exit(0);
        }
    }
    std::fs::write(&ready, forked.to_string()).expect("announce the pipe holder");
    thread::sleep(Duration::from_secs(120));
    std::mem::forget(supervisor);
}

#[cfg(unix)]
#[test]
fn the_reaper_settles_its_group_on_reparenting_without_waiting_for_pipe_eof() {
    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 performs no delivery; it only asks whether the
        // pid can be signalled.
        unsafe { libc::kill(pid, 0) == 0 }
    }
    fn read_pid(path: &std::path::Path, timeout: Duration) -> i32 {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse() {
                    return pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "{} never carried a pid",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    let scratch = std::env::temp_dir().join(format!(
        "upstroke-reparent-{}-{}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    let ready = scratch.join("ready");
    let agent = scratch.join("agent");
    let mut coordinator = Command::new(std::env::current_exe().expect("test executable"))
        .args(["unix_reaper_reparent_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_UNIX_REPARENT", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_AGENT", &agent)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn a disposable coordinator");
    let holder = read_pid(&ready, Duration::from_secs(20));
    let agent_pid = read_pid(&agent, Duration::from_secs(20));
    assert!(alive(agent_pid), "the agent never started");

    coordinator
        .kill()
        .expect("hard-kill the disposable coordinator");
    coordinator.wait().expect("reap the disposable coordinator");
    assert!(
        alive(holder),
        "the pipe holder died with its parent, so no EOF was withheld and \
         this test would pass without the reparenting check"
    );

    let deadline = Instant::now() + Duration::from_secs(20);
    while alive(agent_pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let settled = !alive(agent_pid);
    // SAFETY: cleanup for the failing case, a no-op for the passing one.
    unsafe {
        let _ = libc::kill(holder, libc::SIGKILL);
        let _ = libc::kill(agent_pid, libc::SIGKILL);
        let _ = libc::kill(-agent_pid, libc::SIGKILL);
    }
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        settled,
        "the reaper waited for a pipe EOF that a surviving fork will never \
         deliver: on Darwin that is an agent group nothing settles"
    );
}

#[cfg(windows)]
#[test]
fn a_real_ambient_join_failure_refuses_the_write_command() {
    let error = join_ambient_job_with(&mut NoHooks, || {
        Err("it could not be created (simulated OS failure)".to_owned())
    })
    .expect_err("a failed ambient join must refuse the write command");
    let message = error.to_string();
    assert!(
        message.starts_with(AMBIENT_REFUSAL_PREFIX),
        "the refusal must carry the diagnostic: {message}"
    );
    assert!(
        message.contains("simulated OS failure"),
        "the OS's own reason must survive into the refusal: {message}"
    );
    assert!(
        message.contains("No process was spawned"),
        "the refusal must say nothing was started: {message}"
    );
    assert!(
        !message.contains(AMBIENT_REFUSAL_SIMULATED),
        "a real failure was reported as the injected one: {message}"
    );
    assert!(
        matches!(error, UpstrokeError::Refused { .. }),
        "a refusal, not an agent error: {error:?}"
    );

    join_ambient_job_with(&mut NoHooks, || Ok(())).expect("a successful ambient join proceeds");
}

#[cfg(windows)]
fn windows_descendant_command(ready: &std::path::Path, marker: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["windows_delayed_marker_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_WINDOWS_DESCENDANT", "1")
        .env("UPSTROKE_READY", ready)
        .env("UPSTROKE_MARKER", marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(windows)]
fn windows_tree_scratch(tag: &str) -> std::path::PathBuf {
    let scratch = std::env::temp_dir().join(format!(
        "upstroke-windows-job-{tag}-{}-{}",
        std::process::id(),
        crate::ulid::ulid()
    ));
    std::fs::create_dir_all(&scratch).expect("create Windows job scratch directory");
    scratch
}

#[cfg(windows)]
fn wait_for_marker(path: &std::path::Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "{} was not created", path.display());
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn windows_delayed_marker_helper() {
    if std::env::var_os("UPSTROKE_WINDOWS_DESCENDANT").is_none() {
        return;
    }
    let ready = std::env::var_os("UPSTROKE_READY").expect("ready path");
    let marker = std::env::var_os("UPSTROKE_MARKER").expect("marker path");
    std::fs::write(ready, b"ready").expect("announce descendant start");
    thread::sleep(Duration::from_secs(1));
    std::fs::write(marker, b"leaked").expect("write delayed marker");
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
#[allow(clippy::zombie_processes)]
fn windows_direct_exit_parent_helper() {
    if std::env::var_os("UPSTROKE_WINDOWS_DIRECT_PARENT").is_none() {
        return;
    }
    let ready = std::path::PathBuf::from(std::env::var_os("UPSTROKE_READY").expect("ready path"));
    let marker =
        std::path::PathBuf::from(std::env::var_os("UPSTROKE_MARKER").expect("marker path"));
    windows_descendant_command(&ready, &marker)
        .spawn()
        .expect("spawn ordinary descendant");
    wait_for_marker(&ready, Duration::from_secs(10));
}

#[cfg(windows)]
#[test]
fn successful_direct_exit_kills_windows_descendants() {
    let scratch = windows_tree_scratch("direct-exit");
    let ready = scratch.join("ready");
    let marker = scratch.join("marker");
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "windows_direct_exit_parent_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("UPSTROKE_WINDOWS_DIRECT_PARENT", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_MARKER", &marker);

    let output =
        run_with_timeout(command, "", Duration::from_secs(20)).expect("supervise direct-exit tree");
    assert_eq!(output.code, Some(0), "supervised output: {output:?}");
    assert!(ready.exists(), "the descendant never began executing");
    thread::sleep(Duration::from_millis(1300));
    let leaked = marker.exists();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        !leaked,
        "a Windows descendant outlived its successful parent"
    );
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn windows_job_owner_helper() {
    if std::env::var_os("UPSTROKE_WINDOWS_JOB_OWNER").is_none() {
        return;
    }
    let ready = std::path::PathBuf::from(std::env::var_os("UPSTROKE_READY").expect("ready path"));
    let marker =
        std::path::PathBuf::from(std::env::var_os("UPSTROKE_MARKER").expect("marker path"));
    let command = windows_descendant_command(&ready, &marker);
    let _ = run_with_timeout(command, "", Duration::from_secs(30));
}

#[cfg(windows)]
#[test]
fn kill_on_close_cleans_windows_descendants_after_conductor_death() {
    let scratch = windows_tree_scratch("kill-on-close");
    let ready = scratch.join("ready");
    let marker = scratch.join("marker");
    let mut owner = Command::new(std::env::current_exe().expect("test executable"))
        .args(["windows_job_owner_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_WINDOWS_JOB_OWNER", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_MARKER", &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn disposable job owner");
    wait_for_marker(&ready, Duration::from_secs(10));
    owner.kill().expect("hard-kill disposable job owner");
    owner.wait().expect("reap disposable job owner");

    thread::sleep(Duration::from_millis(1300));
    let leaked = marker.exists();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        !leaked,
        "kill-on-close did not terminate the owned descendant"
    );
}

#[cfg(windows)]
fn windows_self_identity() -> String {
    let pid = std::process::id();
    let created = process_creation_time(pid).expect("this process has a creation time");
    format!("{pid} {created}")
}

#[cfg(windows)]
fn read_windows_identity(path: &std::path::Path, timeout: Duration) -> (u32, u64) {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            let mut fields = text.split_whitespace();
            if let (Some(pid), Some(created)) = (fields.next(), fields.next()) {
                if let (Ok(pid), Ok(created)) = (pid.parse(), created.parse()) {
                    return (pid, created);
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "{} never carried a process identity",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
fn windows_escape_watcher_helper() {
    use std::io::Write;

    if std::env::var_os("UPSTROKE_WINDOWS_WATCHER").is_none() {
        return;
    }
    let ready = std::env::var_os("UPSTROKE_READY").expect("ready path");
    let parent: u32 = std::env::var("UPSTROKE_PARENT_PID")
        .expect("parent pid")
        .parse()
        .expect("parent pid");
    let created: u64 = std::env::var("UPSTROKE_PARENT_CREATED")
        .expect("parent creation time")
        .parse()
        .expect("parent creation time");
    std::fs::write(ready, windows_self_identity()).expect("announce the watcher");
    let mut gone = 0_u8;
    for _ in 0..2000 {
        if process_alive(parent, created) {
            gone = 0;
        } else {
            gone += 1;
        }
        if gone >= 3 {
            eprint!("ESCAPED");
            let _ = std::io::stderr().flush();
            thread::sleep(Duration::from_secs(90));
            return;
        }
        thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(windows)]
#[test]
#[ignore = "subprocess helper"]
#[allow(clippy::zombie_processes)]
fn windows_escape_parent_helper() {
    use std::io::Write;

    if std::env::var_os("UPSTROKE_WINDOWS_ESCAPE_PARENT").is_none() {
        return;
    }
    let ready = std::path::PathBuf::from(std::env::var_os("UPSTROKE_READY").expect("ready path"));
    let pid = std::process::id();
    let created = process_creation_time(pid).expect("own creation time");
    Command::new(std::env::current_exe().expect("test executable"))
        .args(["windows_escape_watcher_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_WINDOWS_WATCHER", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_PARENT_PID", pid.to_string())
        .env("UPSTROKE_PARENT_CREATED", created.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the escape watcher");
    wait_for_marker(&ready, Duration::from_secs(20));
    if std::env::var_os("UPSTROKE_MODE").is_some_and(|mode| mode == "flood") {
        let block = vec![b'x'; 8192];
        let mut out = std::io::stdout();
        while out.write_all(&block).is_ok() && out.flush().is_ok() {}
        return;
    }
    thread::sleep(Duration::from_secs(60));
}

#[cfg(windows)]
fn still_running_after(pid: u32, created: u64, bound: Duration) -> bool {
    let deadline = Instant::now() + bound;
    while process_alive(pid, created) {
        if Instant::now() >= deadline {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

#[cfg(windows)]
fn windows_escape_command(ready: &std::path::Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["windows_escape_parent_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_WINDOWS_ESCAPE_PARENT", "1")
        .env("UPSTROKE_READY", ready)
        .env("UPSTROKE_MODE", mode);
    command
}

#[cfg(windows)]
#[test]
fn kill_tree_observes_the_windows_job_empty_before_it_returns() {
    let scratch = windows_tree_scratch("kill-tree");
    let ready = scratch.join("ready");
    let mut command = windows_escape_command(&ready, "sleep");
    command.stdin(Stdio::null());
    let mut tree = ProcessTree::spawn(&mut command, &mut NoHooks).expect("spawn a tree");
    let (pid, created) = read_windows_identity(&ready, Duration::from_secs(30));
    assert!(process_alive(pid, created), "the grandchild never ran");
    assert_eq!(
        tree.job.contains(tree.child.id()),
        Some(true),
        "the direct child is not in the job that owns its tree"
    );
    if tree.job.contains(std::process::id()) != Some(false) {
        std::mem::forget(tree);
        let _ = std::fs::remove_dir_all(&scratch);
        panic!(
            "the coordinator is a member of the per-invocation job: a timeout \
             on one invocation would terminate the coordinator and every other \
             invocation with it"
        );
    }

    kill_tree(ProcessSite::Terminate, &mut tree).expect("settle the tree");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut escaped = process_alive(pid, created);
    while escaped && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
        escaped = process_alive(pid, created);
    }
    let in_job = tree.job.contains(pid);
    drop(tree);
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        !escaped,
        "kill_tree returned while a member of the job was still running \
         (grandchild in this job: {in_job:?}): the job was never observed empty"
    );
}

#[cfg(windows)]
#[test]
fn timeout_kills_a_windows_grandchild_before_it_can_escape() {
    let scratch = windows_tree_scratch("timeout-escape");
    let ready = scratch.join("ready");
    let output = run_with_timeout(
        windows_escape_command(&ready, "sleep"),
        "",
        Duration::from_secs(3),
    )
    .expect("supervise the tree");
    let (pid, created) = read_windows_identity(&ready, Duration::from_secs(30));
    let escaped = still_running_after(pid, created, Duration::from_secs(3));
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(output.timed_out, "{output:?}");
    assert!(
        !output.stderr.contains("ESCAPED"),
        "a Windows grandchild outlived its timed-out tree: {}",
        output.stderr
    );
    assert!(
        !escaped,
        "the grandchild was still running when the supervisor returned"
    );
}

#[cfg(windows)]
#[test]
fn the_output_limit_path_settles_a_windows_grandchild_too() {
    let scratch = windows_tree_scratch("limit-escape");
    let ready = scratch.join("ready");
    let output = run_with_timeout_and_limit(
        ProcessSite::Spawn,
        ProcessSite::Terminate,
        windows_escape_command(&ready, "flood"),
        b"",
        Duration::from_secs(60),
        64 * 1024,
        &mut NoHooks,
    )
    .expect("supervise the tree");
    let (pid, created) = read_windows_identity(&ready, Duration::from_secs(30));
    let escaped = still_running_after(pid, created, Duration::from_secs(3));
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(output.output_limited, "{output:?}");
    assert!(
        !output.stderr.contains("ESCAPED"),
        "a Windows grandchild outlived an output-limited tree: {}",
        output.stderr
    );
    assert!(
        !escaped,
        "the grandchild was still running when the supervisor returned"
    );
}

#[cfg(windows)]
#[test]
fn every_windows_containment_point_is_measured_against_its_own_operation() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows_sys::Win32::Foundation::HANDLE;

    #[derive(Debug, PartialEq, Eq)]
    struct Row {
        point: SubEffectPoint,
        suspended: bool,
        assignment_made: bool,
        in_private_job: Option<bool>,
        first_instruction_ran: Option<bool>,
    }

    struct Shared {
        pid: Option<u32>,
        job: Option<HANDLE>,
        first_instruction: std::path::PathBuf,
        rows: Vec<Row>,
    }

    struct Observer(Rc<RefCell<Shared>>);

    impl SpawnHooks for Observer {
        fn child_created(&mut self, pid: u32) {
            self.0.borrow_mut().pid = Some(pid);
        }

        fn point(&mut self, point: SubEffectPoint) -> Injection {
            if point != SubEffectPoint::Resumed {
                thread::sleep(Duration::from_millis(250));
            }
            let mut shared = self.0.borrow_mut();
            let pid = shared.pid.expect("the child exists at every point");
            let job = shared.job;
            let suspended = windows_job::primary_thread_suspend_count(pid)
                .expect("read the child's suspend count")
                > 0;
            let first_instruction_ran = if point == SubEffectPoint::Resumed {
                None
            } else {
                Some(shared.first_instruction.exists())
            };
            let row = Row {
                point,
                suspended,
                assignment_made: job.is_some(),
                in_private_job: job.and_then(|job| windows_job::job_contains(job, pid)),
                first_instruction_ran,
            };
            shared.rows.push(row);
            Injection::Proceed
        }
    }

    let scratch = windows_tree_scratch("point-coordinates");
    let ready = scratch.join("ready");
    let marker = scratch.join("marker");
    let shared = Rc::new(RefCell::new(Shared {
        pid: None,
        job: None,
        first_instruction: ready.clone(),
        rows: Vec::new(),
    }));
    let mut command = windows_descendant_command(&ready, &marker);
    let mut hooks = Observer(Rc::clone(&shared));
    let assign_shared = Rc::clone(&shared);
    let (mut child, job) = windows_job::spawn_suspended_in_job_with(
        &mut command,
        &mut hooks,
        move |job, process| {
            assign_shared.borrow_mut().job = Some(job);
            windows_job::real_assign_to_job(job, process)
        },
        windows_job::resume_only_thread,
    )
    .expect("spawn a suspended child");

    wait_for_marker(&ready, Duration::from_secs(20));
    let _ = job.terminate_and_wait();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&scratch);

    let observed = &shared.borrow().rows;
    let expected = vec![
        Row {
            point: SubEffectPoint::CreatedSuspended,
            suspended: true,
            assignment_made: false,
            in_private_job: None,
            first_instruction_ran: Some(false),
        },
        Row {
            point: SubEffectPoint::PrivateJobAssigned,
            suspended: true,
            assignment_made: true,
            in_private_job: Some(true),
            first_instruction_ran: Some(false),
        },
        Row {
            point: SubEffectPoint::Resumed,
            suspended: false,
            assignment_made: true,
            in_private_job: Some(true),
            first_instruction_ran: None,
        },
    ];
    assert_eq!(
        *observed, expected,
        "a containment point no longer sits at the coordinate it names"
    );
}

#[cfg(windows)]
#[test]
fn a_windows_spawn_that_fails_after_creation_leaves_no_suspended_stub() {
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Capture(Rc<RefCell<Option<(u32, u64)>>>);

    impl SpawnHooks for Capture {
        fn point(&mut self, _point: SubEffectPoint) -> Injection {
            Injection::Proceed
        }

        fn child_created(&mut self, pid: u32) {
            let created = process_creation_time(pid).expect("the child has a creation time");
            *self.0.borrow_mut() = Some((pid, created));
        }
    }

    for (step, assign, resume) in [
        (
            "private-job assignment",
            None,
            None::<fn(u32) -> std::io::Result<()>>,
        ),
        (
            "resume",
            Some(windows_job::real_assign_to_job as fn(_, _) -> i32),
            Some(|_| Err(std::io::Error::other("simulated resume failure"))),
        ),
    ] {
        let scratch = windows_tree_scratch("spawn-failure");
        let ready = scratch.join("ready");
        let marker = scratch.join("marker");
        let seen = Rc::new(RefCell::new(None));
        let mut hooks = Capture(Rc::clone(&seen));
        let mut command = windows_descendant_command(&ready, &marker);
        let error = windows_job::spawn_suspended_in_job_with(
            &mut command,
            &mut hooks,
            move |job, process| assign.map_or(0, |assign| assign(job, process)),
            move |pid| resume.map_or_else(|| windows_job::resume_only_thread(pid), |r| r(pid)),
        )
        .err()
        .unwrap_or_else(|| panic!("a failed {step} must be a spawn failure"));
        let (pid, created) = seen
            .borrow()
            .unwrap_or_else(|| panic!("the child was created before the {step}"));
        let alive = process_alive(pid, created);
        let ran = ready.exists();
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(
            !alive,
            "a suspended stub outlived the failed {step} ({error}): pid {pid} is still running"
        );
        assert!(
            !ran,
            "the child executed although the {step} it was waiting behind failed"
        );
    }
}

#[cfg(unix)]
#[test]
fn successful_direct_exit_still_kills_detached_group_members() {
    let marker = std::env::temp_dir().join(format!(
        "upstroke-proc-detached-{}-{}.marker",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&marker);
    let mut command = shell(
        "(sleep 1; printf leaked > \"$UPSTROKE_MARKER\") \
         </dev/null >/dev/null 2>&1 & exit 0",
    );
    command.env("UPSTROKE_MARKER", &marker);
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
    if std::env::var_os("UPSTROKE_SIGNAL_WORKER").is_none() {
        return;
    }
    let ready = std::env::var_os("UPSTROKE_READY").expect("ready path");
    let marker = std::env::var_os("UPSTROKE_MARKER").expect("marker path");
    let finish = std::env::var_os("UPSTROKE_FINISH").expect("finish path");
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

#[cfg(unix)]
#[test]
fn a_stopped_child_is_not_mistaken_for_an_exited_child() {
    let scratch = std::env::temp_dir().join(format!(
        "upstroke-stopped-child-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let ready = scratch.join("ready");
    let marker = scratch.join("marker");
    let finish = scratch.join("finish");
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "terminal_progress_worker_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("UPSTROKE_SIGNAL_WORKER", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_MARKER", &marker)
        .env("UPSTROKE_FINISH", &finish)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn stopped-child helper");
    let pid = i32::try_from(child.id()).expect("child pid");
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < ready_deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "stopped-child helper never became ready");
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);

    let stop_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WSTOPPED | libc::WNOHANG | libc::WNOWAIT,
                )
            },
            0
        );
        if unsafe { info.si_pid() } == pid {
            break;
        }
        assert!(Instant::now() < stop_deadline, "child never stopped");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !child_exited_unreaped(&child).expect("probe stopped child"),
        "a non-terminal child transition was mistaken for process exit"
    );

    let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(scratch);
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess helper"]
fn terminal_interrupt_helper() {
    if std::env::var_os("UPSTROKE_SIGNAL_HELPER").is_none() {
        return;
    }
    let no_core = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: this changes only the current disposable helper before it
    // launches either the signal monitor or the supervised command.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &no_core) }, 0);
    let _cleanup_lock = std::env::var_os("UPSTROKE_CLEANUP_PUBLIC").map(|public| {
        let public = std::path::PathBuf::from(public);
        std::fs::create_dir_all(&public).expect("cleanup-lock run directory");
        crate::rundir::RunLock::acquire(&public).expect("cleanup-lock helper takes run")
    });
    let _cleanup_scope = _cleanup_lock
        .as_ref()
        .map(crate::rundir::RunLock::enter_cleanup_scope);
    if let Some(blocked_signal) = std::env::var_os("UPSTROKE_BLOCK_SIGNAL") {
        // SAFETY: this disposable process deliberately models an embedding
        // host that blocked the selected signal before Upstroke initialized
        // supervision.
        let blocked_signal = blocked_signal
            .to_string_lossy()
            .parse::<libc::c_int>()
            .expect("numeric blocked signal");
        unsafe {
            let mut blocked: libc::sigset_t = std::mem::zeroed();
            assert_eq!(libc::sigemptyset(&mut blocked), 0);
            assert_eq!(libc::sigaddset(&mut blocked, blocked_signal), 0);
            assert_eq!(
                libc::sigprocmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()),
                0
            );
        }
    }
    let custom_handler = std::env::var_os("UPSTROKE_CUSTOM_SIGNAL_HANDLER").is_some();
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
    let custom_job_control = std::env::var_os("UPSTROKE_CUSTOM_JOB_CONTROL_HANDLER").is_some();
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
    if std::env::var_os("UPSTROKE_CUSTOM_CONTINUE_HANDLER").is_some() {
        assert_ne!(
            unsafe {
                libc::signal(
                    libc::SIGCONT,
                    record_custom_continue as *const () as libc::sighandler_t,
                )
            },
            libc::SIG_ERR
        );
    }
    let custom_aux_signal = std::env::var_os("UPSTROKE_CUSTOM_AUX_SIGNAL_HANDLER").is_some();
    if custom_aux_signal {
        CUSTOM_AUX_SIGNAL_SEEN.store(false, std::sync::atomic::Ordering::SeqCst);
        CUSTOM_PARENT_PID.store(
            unsafe { libc::getpid() },
            std::sync::atomic::Ordering::SeqCst,
        );
        assert_ne!(
            unsafe {
                libc::signal(
                    libc::SIGUSR1,
                    record_custom_aux_signal as *const () as libc::sighandler_t,
                )
            },
            libc::SIG_ERR
        );
    }
    let progress_loop = std::env::var_os("UPSTROKE_SIGNAL_PROGRESS_LOOP").is_some();
    let mut command = if progress_loop {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command.args([
            "terminal_progress_worker_helper",
            "--ignored",
            "--nocapture",
        ]);
        command.env("UPSTROKE_SIGNAL_WORKER", "1");
        command
    } else {
        let script = "(sleep 1; printf leaked > \"$UPSTROKE_MARKER\") & worker=$!; \
         shell_pgid=$(ps -o pgid= -p $$ | tr -d ' '); \
         worker_pgid=$(ps -o pgid= -p $worker | tr -d ' '); \
         printf '%s %s %s %s' $$ $shell_pgid $worker $worker_pgid > \"$UPSTROKE_READY\"; \
         wait";
        shell(script)
    };
    command.env(
        "UPSTROKE_READY",
        std::env::var_os("UPSTROKE_READY").expect("ready path"),
    );
    command.env(
        "UPSTROKE_MARKER",
        std::env::var_os("UPSTROKE_MARKER").expect("marker path"),
    );
    if let Some(finish) = std::env::var_os("UPSTROKE_FINISH") {
        command.env("UPSTROKE_FINISH", finish);
    }
    let result = run_with_timeout(command, "", Duration::from_secs(30));
    if std::env::var_os("UPSTROKE_EXPECT_JOB_CONTROL_REFUSAL").is_some() {
        let error = result.expect_err("host-owned SIGCONT must refuse default stop proxying");
        assert!(
            error
                .to_string()
                .contains("cannot safely proxy default Unix job-control stops"),
            "unexpected policy error: {error}"
        );
        return;
    }
    let output = result.expect("signal helper command");
    if std::env::var_os("UPSTROKE_SIGNAL_HELPER_EXPECT_RETURN").is_some() {
        assert_eq!(output.code, Some(0), "supervised output: {output:?}");
        if custom_handler {
            assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
            assert!(
                CUSTOM_SIGNAL_SEEN.load(std::sync::atomic::Ordering::SeqCst),
                "Upstroke replaced the embedding host's custom SIGTERM handler"
            );
        }
        if custom_job_control {
            assert!(
                CUSTOM_JOB_CONTROL_SEEN.load(std::sync::atomic::Ordering::SeqCst),
                "Upstroke replaced the embedding host's custom SIGTSTP handler"
            );
        }
        if custom_aux_signal {
            assert!(
                CUSTOM_AUX_SIGNAL_SEEN.load(std::sync::atomic::Ordering::SeqCst),
                "the embedding host did not receive its own SIGUSR1"
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
static CUSTOM_AUX_SIGNAL_SEEN: std::sync::atomic::AtomicBool =
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
        let _ = unsafe { libc::kill(parent, libc::SIGKILL) };
    }
}

#[cfg(unix)]
extern "C" fn record_custom_continue(_: libc::c_int) {}

#[cfg(unix)]
extern "C" fn record_custom_aux_signal(_: libc::c_int) {
    let parent = CUSTOM_PARENT_PID.load(std::sync::atomic::Ordering::SeqCst);
    if unsafe { libc::getpid() } == parent {
        CUSTOM_AUX_SIGNAL_SEEN.store(true, std::sync::atomic::Ordering::SeqCst);
    } else if parent > 0 {
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
    reaper_pid_path: std::path::PathBuf,
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
        "upstroke-proc-{tag}-{}-{}-{}",
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
    let reaper_pid_path = scratch.join("reaper.pid");
    let diagnostic_stdout = std::fs::File::create(&diagnostic).expect("helper diagnostic");
    let diagnostic_stderr = diagnostic_stdout
        .try_clone()
        .expect("clone helper diagnostic");

    let mut helper = Command::new(std::env::current_exe().expect("test executable"));
    helper
        .args(["terminal_interrupt_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_SIGNAL_HELPER", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_MARKER", &marker)
        .env("UPSTROKE_FINISH", &finish)
        .env("UPSTROKE_TEST_REAPER_PID_PATH", &reaper_pid_path)
        .process_group(0)
        .stdout(Stdio::from(diagnostic_stdout))
        .stderr(Stdio::from(diagnostic_stderr));
    if expect_return {
        helper.env("UPSTROKE_SIGNAL_HELPER_EXPECT_RETURN", "1");
    }
    if tag.starts_with("job-control") || tag == "crash-lease" {
        helper.env("UPSTROKE_SIGNAL_PROGRESS_LOOP", "1");
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
        helper.env("UPSTROKE_CUSTOM_SIGNAL_HANDLER", "1");
    }
    if tag == "custom-job-control" {
        helper.env("UPSTROKE_CUSTOM_JOB_CONTROL_HANDLER", "1");
    }
    if tag == "custom-aux-signal" {
        helper.env("UPSTROKE_CUSTOM_AUX_SIGNAL_HANDLER", "1");
    }
    let blocked_signal = if tag == "job-control-cont-blocked" {
        Some(libc::SIGCONT)
    } else if tag.contains("blocked") {
        Some(libc::SIGTERM)
    } else {
        None
    };
    if let Some(blocked_signal) = blocked_signal {
        helper.env("UPSTROKE_BLOCK_SIGNAL", blocked_signal.to_string());
        unsafe {
            helper.pre_exec(move || {
                let mut blocked: libc::sigset_t = std::mem::zeroed();
                if libc::sigemptyset(&mut blocked) != 0
                    || libc::sigaddset(&mut blocked, blocked_signal) != 0
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
            .env("UPSTROKE_CLEANUP_PUBLIC", scratch.join("run"))
            .env("UPSTROKE_TEST_CLEANUP_DELAY_MS", "700");
    }
    let child = helper.spawn().expect("spawn signal helper");
    let mut helper = SignalHelper {
        child,
        scratch,
        marker,
        finish,
        diagnostic,
        reaper_pid_path,
        supervised_pgid: None,
        active: true,
    };

    let ready_deadline = Instant::now() + Duration::from_secs(10);
    let mut last_identities = String::new();
    let identities = loop {
        if let Some(status) = helper.child.try_wait().expect("poll helper") {
            panic!("signal helper exited before its child was ready: {status}");
        }
        if let Ok(current) = std::fs::read_to_string(&ready) {
            if current.split_whitespace().count() == 4 {
                break current;
            }
            last_identities = current;
        }
        if Instant::now() >= ready_deadline {
            panic!(
                "signal helper never published complete child identities; last payload: \
                 {last_identities:?}"
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
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
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
        assert!(waited >= 0, "waitpid: {}", std::io::Error::last_os_error());
        if waited == pid && libc::WIFSTOPPED(status) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

#[cfg(unix)]
fn wait_for_first_progress(marker: &std::path::Path, context: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if marker.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("the supervised worker never recorded progress before {context}");
}

#[cfg(unix)]
fn settled_progress_after_stop(marker: &std::path::Path, context: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut previous = std::fs::read_to_string(marker)
        .unwrap_or_else(|error| panic!("progress before {context}: {error}"));
    loop {
        thread::sleep(Duration::from_millis(125));
        let current = std::fs::read_to_string(marker)
            .unwrap_or_else(|error| panic!("progress while settling {context}: {error}"));
        if current == previous {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "the isolated agent never became quiescent during {context}: {previous} -> {current}"
        );
        previous = current;
    }
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
        "signal {signal} terminated Upstroke but left its isolated agent tree alive"
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
fn a_host_owned_sigcont_rejects_default_stop_proxying_before_launch() {
    use std::os::unix::process::CommandExt;

    let scratch = std::env::temp_dir().join(format!(
        "upstroke-proc-custom-cont-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).expect("custom-cont scratch");
    let ready = scratch.join("ready");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["terminal_interrupt_helper", "--ignored", "--nocapture"])
        .env("UPSTROKE_SIGNAL_HELPER", "1")
        .env("UPSTROKE_CUSTOM_CONTINUE_HANDLER", "1")
        .env("UPSTROKE_EXPECT_JOB_CONTROL_REFUSAL", "1")
        .env("UPSTROKE_READY", &ready)
        .env("UPSTROKE_MARKER", scratch.join("marker"))
        .env("UPSTROKE_FINISH", scratch.join("finish"))
        .process_group(0)
        .output()
        .expect("run custom-SIGCONT policy helper");
    assert!(
        output.status.success(),
        "custom-SIGCONT helper failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !ready.exists(),
        "an agent launched under the unsafe signal policy"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

#[cfg(unix)]
#[test]
fn arbitrary_host_callbacks_never_run_in_private_helpers() {
    let mut helper = spawn_signal_helper("custom-aux-signal", true, false);
    let parent = helper.pid();
    let reaper: i32 = std::fs::read_to_string(&helper.reaper_pid_path)
        .expect("recorded private reaper pid")
        .trim()
        .parse()
        .expect("numeric private reaper pid");

    assert_eq!(unsafe { libc::kill(-parent, libc::SIGUSR1) }, 0);
    assert_eq!(unsafe { libc::kill(reaper, libc::SIGUSR1) }, 0);
    thread::sleep(Duration::from_millis(50));
    std::fs::write(&helper.finish, "finish").expect("release supervised worker");

    let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
        .expect("host-callback helper completes normally");
    let diagnostic = helper.diagnostic();
    helper.complete();
    assert!(status.success(), "helper status: {status}\n{diagnostic}");
}

#[cfg(unix)]
#[test]
fn sigkill_of_upstroke_job_still_reaps_the_isolated_agent_group() {
    let mut helper = spawn_signal_helper("job-control", true, false);
    let helper_pgid = helper.pid();
    let agent_pgid = helper.supervised_pgid.expect("supervised group");
    assert_eq!(unsafe { libc::kill(-helper_pgid, libc::SIGKILL) }, 0);
    wait_for_exit(&mut helper.child, Duration::from_secs(10))
        .expect("SIGKILLed helper exits promptly");

    helper.active = false;
    thread::sleep(Duration::from_millis(1300));
    let before = std::fs::read_to_string(&helper.marker).ok();
    thread::sleep(Duration::from_millis(350));
    let after = std::fs::read_to_string(&helper.marker).ok();
    let stopped = before == after;

    let _ = unsafe { libc::kill(-agent_pgid, libc::SIGKILL) };
    helper.complete();
    assert!(
        stopped,
        "the isolated agent kept running after an uncatchable Upstroke SIGKILL"
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
fn assert_stop_covers_the_isolated_tree(signal: libc::c_int, tag: &str) {
    let mut helper = spawn_signal_helper(tag, true, false);
    let pid = helper.pid();
    wait_for_first_progress(&helper.marker, &format!("signal {signal}"));
    assert_eq!(unsafe { libc::kill(-pid, signal) }, 0);
    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not stop for signal {signal}"
    );

    let before = settled_progress_after_stop(&helper.marker, &format!("signal {signal}"));
    thread::sleep(Duration::from_millis(350));
    let after = std::fs::read_to_string(&helper.marker)
        .unwrap_or_else(|error| panic!("progress after signal {signal}: {error}"));
    assert_eq!(
        after, before,
        "the isolated agent kept making progress while Upstroke was stopped by signal {signal}"
    );

    std::fs::write(&helper.finish, "finish").expect("release supervised worker");
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
    let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
        .unwrap_or_else(|| panic!("signal {signal} left the supervised tree stranded"));
    let diagnostic = helper.diagnostic();
    helper.complete();
    assert!(status.success(), "helper status: {status}\n{diagnostic}");
}

#[cfg(unix)]
#[test]
fn terminal_input_and_output_stops_cover_the_isolated_tree() {
    for (signal, tag) in [
        (libc::SIGTTIN, "job-control-ttin"),
        (libc::SIGTTOU, "job-control-ttou"),
    ] {
        assert_stop_covers_the_isolated_tree(signal, tag);
    }
}

#[cfg(unix)]
#[test]
fn uncatchable_sigstop_covers_the_isolated_tree() {
    assert_stop_covers_the_isolated_tree(libc::SIGSTOP, "job-control-sigstop");
}

#[cfg(unix)]
#[test]
fn terminal_suspend_and_continue_cover_the_isolated_tree() {
    let mut helper = spawn_signal_helper("job-control", true, false);
    let pid = helper.pid();
    wait_for_first_progress(&helper.marker, "suspend interval");
    // SAFETY: `pid` is the id of the helper's dedicated process group, so
    // this models terminal foreground-group job control without touching
    // the surrounding test runner.
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);

    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not enter a stopped job-control state"
    );

    let before = settled_progress_after_stop(&helper.marker, "suspend interval");
    thread::sleep(Duration::from_millis(350));
    let after = std::fs::read_to_string(&helper.marker).expect("progress after suspend interval");
    assert_eq!(
        after, before,
        "the isolated agent kept making progress while Upstroke was suspended"
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
    assert!(
        resumed,
        "the isolated agent was not continued with Upstroke"
    );
}

#[cfg(unix)]
#[test]
fn an_inherited_blocked_sigcont_still_releases_the_isolated_tree() {
    let mut helper = spawn_signal_helper("job-control-cont-blocked", true, false);
    let pid = helper.pid();
    wait_for_first_progress(&helper.marker, "blocked SIGCONT");
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not enter a stopped job-control state"
    );

    let before = settled_progress_after_stop(&helper.marker, "blocked SIGCONT");
    thread::sleep(Duration::from_millis(350));
    let after = std::fs::read_to_string(&helper.marker).expect("progress after blocked SIGCONT");
    assert_eq!(
        after, before,
        "the isolated agent kept making progress while Upstroke was suspended"
    );

    std::fs::write(&helper.finish, "finish").expect("release supervised worker");
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
    let status = wait_for_exit(&mut helper.child, Duration::from_secs(10))
        .expect("blocked SIGCONT stranded Upstroke or its isolated agent tree");
    let diagnostic = helper.diagnostic();
    helper.complete();
    assert!(status.success(), "helper status: {status}\n{diagnostic}");
}

#[cfg(unix)]
#[test]
fn a_blocked_terminal_signal_still_wakes_a_suspended_host() {
    let mut helper = spawn_signal_helper("job-control-blocked", true, false);
    let pid = helper.pid();
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not enter a stopped job-control state"
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
        "Upstroke did not enter a stopped job-control state"
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
    wait_for_first_progress(&helper.marker, "ignored SIGHUP");
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not enter a stopped job-control state"
    );
    let before = settled_progress_after_stop(&helper.marker, "ignored SIGHUP");
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
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGCONT) }, 0);
    std::fs::write(&helper.finish, "finish").expect("release supervised worker");

    let status = wait_for_exit(&mut helper.child, Duration::from_secs(10)).unwrap_or_else(|| {
        panic!("a continue racing with suspend stranded Upstroke or its agent tree");
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
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTERM) }, 0);
    if wait_for_exit(&mut helper.child, Duration::from_secs(10)).is_none() {
        panic!("termination racing with suspend did not terminate Upstroke");
    }
    thread::sleep(Duration::from_millis(1300));
    let leaked = helper.marker.exists();
    helper.complete();
    assert!(!leaked, "the suspended agent tree survived termination");
}

#[cfg(unix)]
#[test]
fn pid_directed_termination_kills_a_suspended_tree_without_continue() {
    let mut helper = spawn_signal_helper("pid-suspend-termination", false, false);
    let pid = helper.pid();
    assert_eq!(unsafe { libc::kill(-pid, libc::SIGTSTP) }, 0);
    assert!(
        wait_for_stop(pid, Duration::from_secs(10)),
        "Upstroke did not enter a stopped job-control state"
    );

    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    wait_for_exit(&mut helper.child, Duration::from_secs(10))
        .expect("PID-directed termination did not release the stopped Upstroke process");
    thread::sleep(Duration::from_millis(1300));
    let leaked = helper.marker.exists();
    helper.complete();
    assert!(
        !leaked,
        "the isolated agent tree survived PID-directed termination"
    );
}

#[test]
fn missing_binary_is_a_spawn_error() {
    let cmd = Command::new("upstroke-definitely-not-a-real-binary");
    let err = run_with_timeout(cmd, "", Duration::from_secs(1)).expect_err("must fail");
    assert!(err.to_string().contains("failed to spawn"));
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess helper"]
#[allow(clippy::zombie_processes)]
fn unix_reaper_container_helper() {
    if std::env::var_os("UPSTROKE_REAPER_CONTAINERS").is_none() {
        return;
    }
    let stub = std::path::PathBuf::from(std::env::var_os("UPSTROKE_STUB").expect("stub path"));
    let root = std::path::PathBuf::from(std::env::var_os("UPSTROKE_ROOT").expect("root"));
    let incarnation = std::env::var("UPSTROKE_INCARNATION").expect("incarnation");
    let agent = std::path::PathBuf::from(std::env::var_os("UPSTROKE_AGENT").expect("agent path"));

    let scope =
        crate::runner::container::census::ReaperContainerScope::new(stub, &root, &incarnation)
            .expect("a scope");
    super::set_container_reclaim_scope(Some(&scope)).expect("arm the reaper");

    let mut supervisor =
        termination::Supervisor::begin(ProcessSite::Terminate).expect("start a private reaper");
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "sleep 120"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    supervisor.prepare(&mut command);
    let child = command.spawn().expect("spawn an agent in its own group");
    supervisor
        .register(child.id())
        .expect("register the agent group");
    std::fs::write(&agent, child.id().to_string()).expect("record the agent pid");
    if std::env::var_os("UPSTROKE_REAPER_CONTAINERS_CLEAN_EXIT").is_some() {
        drop(supervisor);
        return;
    }
    thread::sleep(Duration::from_secs(120));
    std::mem::forget(supervisor);
}

#[cfg(unix)]
#[test]
fn unix_reaper_kills_labeled_containers() {
    const CONTAINER_ID: &str = "c0ffee0000000000000000000000000000000000000000000000000000000001";
    const PRIVATE_ROOT: &str = "/srv/upstroke-reaper-fixture/private";
    const INCARNATION: &str = "01KZTAAAAAAAAAAAAAAAAAAAAA";

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "upstroke-reaper-containers-{tag}-{}-{}",
            std::process::id(),
            crate::ulid::ulid()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }
    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 performs no delivery.
        unsafe { libc::kill(pid, 0) == 0 }
    }
    fn read_pid(path: &std::path::Path, timeout: Duration) -> i32 {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse() {
                    return pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "{} never carried a pid",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if path.exists() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    const STUB_NAME: &str = "upstroke-reaper-docker-stub";
    for (bare, coordinator_dies) in [(true, true), (false, true), (false, false)] {
        let cell = match (bare, coordinator_dies) {
            (true, true) => "bare-dies",
            (false, true) => "path-dies",
            _ => "path-lives",
        };
        let dir = scratch(cell);
        let stub = dir.join(STUB_NAME);
        let log = dir.join("argv.log");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$*\" >> \"$UPSTROKE_STUB_DIR/argv.log\"\n\
                 case \"$1\" in\n\
                 ps) [ -f \"$UPSTROKE_STUB_DIR/removed\" ] || printf '%s\\n' '{CONTAINER_ID}' ;;\n\
                 kill) : > \"$UPSTROKE_STUB_DIR/killing\"; sleep 1 ;;\n\
                 rm) : > \"$UPSTROKE_STUB_DIR/removed\" ;;\n\
                 esac\n\
                 exit 0\n"
            )
            .replace("{CONTAINER_ID}", CONTAINER_ID),
        )
        .expect("write the stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                .expect("make the stub executable");
        }

        let agent_path = dir.join("agent");
        let reaper_path = dir.join("reaper");
        let named: std::path::PathBuf = if bare {
            std::path::PathBuf::from(STUB_NAME)
        } else {
            stub.clone()
        };
        let mut coordinator = Command::new(std::env::current_exe().expect("test executable"));
        coordinator
            .args(["unix_reaper_container_helper", "--ignored", "--nocapture"])
            .env("UPSTROKE_REAPER_CONTAINERS", "1")
            .env("UPSTROKE_STUB", &named)
            .env("UPSTROKE_STUB_DIR", &dir)
            .env("UPSTROKE_ROOT", PRIVATE_ROOT)
            .env("UPSTROKE_INCARNATION", INCARNATION)
            .env("UPSTROKE_AGENT", &agent_path)
            .env("UPSTROKE_TEST_REAPER_PID_PATH", &reaper_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if bare {
            let inherited = std::env::var_os("PATH").unwrap_or_default();
            let mut search = vec![dir.clone()];
            search.extend(std::env::split_paths(&inherited));
            coordinator.env(
                "PATH",
                std::env::join_paths(search).expect("a synthetic PATH"),
            );
        }
        if !coordinator_dies {
            coordinator.env("UPSTROKE_REAPER_CONTAINERS_CLEAN_EXIT", "1");
        }
        let mut coordinator = coordinator.spawn().expect("spawn a disposable coordinator");

        let agent_pid = read_pid(&agent_path, Duration::from_secs(30));
        let reaper_pid = read_pid(&reaper_path, Duration::from_secs(30));

        if !coordinator_dies {
            coordinator.wait().expect("reap the coordinator");
            thread::sleep(Duration::from_millis(500));
            assert!(
                !log.exists(),
                "the reaper reclaimed a LIVE coordinator's containers on the ordinary \
                 settle path: {:?}",
                std::fs::read_to_string(&log)
            );
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }

        assert!(alive(agent_pid), "the agent never started");
        coordinator.kill().expect("hard-kill the coordinator");
        coordinator.wait().expect("reap the coordinator");

        assert!(
            wait_for(&dir.join("killing"), Duration::from_secs(30)),
            "[{cell}] the reaper never issued a container kill"
        );
        assert!(
            alive(reaper_pid),
            "the reaper exited — releasing its shared cleanup hold — before the container \
             kill it was in the middle of returned"
        );

        let deadline = Instant::now() + Duration::from_secs(30);
        let lines = loop {
            let lines: Vec<String> = std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect();
            if lines.len() >= 3 || Instant::now() >= deadline {
                break lines;
            }
            thread::sleep(Duration::from_millis(20));
        };
        assert!(
            lines.len() >= 3,
            "[{cell}] the reaper's docker log is {lines:#?}"
        );
        assert!(lines[0].starts_with("ps "), "{lines:#?}");
        assert_eq!(lines[1], format!("kill {CONTAINER_ID}"), "{lines:#?}");
        let declared = crate::runner::container::census::ReaperContainerScope::new(
            "docker",
            std::path::Path::new(PRIVATE_ROOT),
            INCARNATION,
        )
        .expect("a scope")
        .remove_argv(CONTAINER_ID);
        assert_eq!(lines[2], declared[1..].join(" "), "{lines:#?}");

        let filters: Vec<&str> = lines[0]
            .split_whitespace()
            .filter(|word| word.starts_with("label="))
            .collect();
        assert_eq!(
            filters.len(),
            2,
            "the reaper's selector is `{}`; a filter on the private root alone names every \
             container of every run under it, including a live coordinator's",
            lines[0]
        );
        assert!(
            filters
                .iter()
                .any(|filter| *filter == format!("label=upstroke.private_root={PRIVATE_ROOT}"))
        );
        assert!(
            filters
                .iter()
                .any(|filter| *filter == format!("label=upstroke.incarnation={INCARNATION}"))
        );
        assert_eq!(
            filters
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2,
            "two filters carrying one value is one filter"
        );

        let settled_by = Instant::now() + Duration::from_secs(30);
        while alive(agent_pid) && Instant::now() < settled_by {
            thread::sleep(Duration::from_millis(50));
        }
        let settled = !alive(agent_pid);
        // SAFETY: cleanup for the failing case, a no-op for the passing one.
        unsafe {
            let _ = libc::kill(agent_pid, libc::SIGKILL);
            let _ = libc::kill(-agent_pid, libc::SIGKILL);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            settled,
            "the container half replaced the process half: the agent group survived"
        );
    }
}

const READINESS_ROLE: &str = "UPSTROKE_READINESS_ROLE";

const READINESS_SIGNAL: &str = "UPSTROKE_READINESS_SIGNAL";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "upstroke-readiness-{tag}-{}-{}",
            std::process::id(),
            crate::ulid::ulid()
        ));
        std::fs::create_dir_all(&dir).expect("readiness scratch directory");
        Self(dir)
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "subprocess helper"]
fn readiness_producer_helper() {
    let Some(role) = std::env::var_os(READINESS_ROLE) else {
        return;
    };
    const ALIVE: Duration = Duration::from_secs(120);
    const SLOW: Duration = Duration::from_millis(200);
    let signal =
        || PathBuf::from(std::env::var_os(READINESS_SIGNAL).expect("the signal path to publish"));
    match role.to_string_lossy().as_ref() {
        "silent" => thread::sleep(ALIVE),
        "dead" => {}
        "signal-after" => {
            thread::sleep(SLOW);
            readiness::publish(&signal(), &["published"]).expect("publish the signal");
            thread::sleep(ALIVE);
        }
        "line-after" => {
            thread::sleep(SLOW);
            println!("held");
            std::io::stdout().flush().expect("frame the line");
            thread::sleep(ALIVE);
        }
        "noise" => {
            let mut out = std::io::BufWriter::with_capacity(1 << 16, std::io::stdout());
            let record = "not-the-line\n".repeat(512);
            loop {
                if out.write_all(record.as_bytes()).is_err() {
                    break;
                }
            }
        }
        other => panic!("unknown readiness producer role `{other}`"),
    }
    std::process::exit(0);
}

fn readiness_producer(role: &str, signal: Option<&Path>, stdout: Stdio) -> readiness::Producer {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "agent::proc::tests::readiness_producer_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(READINESS_ROLE, role)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null());
    if let Some(signal) = signal {
        command.env(READINESS_SIGNAL, signal);
    }
    readiness::Producer::adopt(command.spawn().expect("spawn the readiness producer"))
}

#[test]
fn a_partial_record_is_refused_rather_than_read_as_a_short_one() {
    let scratch = Scratch::new("partial");
    let signal = scratch.join("signal");

    let torn = "/tmp/upstroke-snapsho";
    std::fs::write(&signal, torn).expect("plant a truncated write");
    assert_eq!(
        std::fs::read_to_string(&signal)
            .expect("read the truncated write")
            .lines()
            .next(),
        Some(torn),
        "the control is sharp: `lines` yields a truncated tail as a whole field, so a \
         reader built on it cannot tell this from a complete record"
    );
    let error = readiness::read_published(&signal).expect_err("a partial record is refused");
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof, "{error}");
    assert!(error.to_string().contains("truncated write"), "{error}");

    readiness::publish(&signal, &["/tmp/upstroke-snapshot", "cafe"]).expect("publish");
    assert_eq!(
        readiness::read_published(&signal).expect("a whole record reads"),
        ["/tmp/upstroke-snapshot", "cafe"]
    );

    let error = readiness::publish(&signal, &["two\nfields"])
        .expect_err("a field carrying the delimiter is refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{error}");
    assert_eq!(
        readiness::read_published(&signal).expect("the refused publish changed nothing"),
        ["/tmp/upstroke-snapshot", "cafe"]
    );
}

#[test]
fn a_live_but_silent_producer_ends_the_wait_at_the_bound() {
    const BOUND: Duration = Duration::from_millis(250);
    let scratch = Scratch::new("silent");
    let signal = scratch.join("signal");

    let mut producer = readiness_producer("silent", Some(&signal), Stdio::null());
    let started = Instant::now();
    let waited = readiness::await_signal(&signal, producer.child(), BOUND);
    let elapsed = started.elapsed();
    match waited {
        readiness::Waited::TimedOut(reported) => assert_eq!(reported, BOUND),
        other => panic!("a live silent producer must time the wait out, not give {other:?}"),
    }
    assert!(
        elapsed >= BOUND,
        "the wait ended before its own bound: {elapsed:?}"
    );
    assert!(
        producer.alive(),
        "the producer must still be running: a wait that only ever ends once its producer \
         has died is not bounding the live-silent case at all"
    );
    drop(producer);

    let mut producer = readiness_producer("silent", None, Stdio::piped());
    let started = Instant::now();
    let waited = producer.await_line("held", BOUND);
    let elapsed = started.elapsed();
    match waited {
        readiness::Waited::TimedOut(reported) => assert_eq!(reported, BOUND),
        other => panic!("a live silent producer must time the wait out, not give {other:?}"),
    }
    assert!(
        elapsed >= BOUND,
        "the wait ended before its own bound: {elapsed:?}"
    );
    assert!(
        producer.alive(),
        "the producer is still holding the pipe open, which is the case a deadline checked \
         after `read_line` returns can never reach"
    );
}

#[test]
fn a_flooding_producer_is_stopped_by_the_output_allowance() {
    const GENEROUS: Duration = Duration::from_secs(30);
    let mut producer = readiness_producer("noise", None, Stdio::piped());
    let started = Instant::now();
    let waited = producer.await_line("held", GENEROUS);
    let elapsed = started.elapsed();
    match waited {
        readiness::Waited::Torn(why) => assert!(
            why.contains("output allowance"),
            "the allowance must be what it names: {why}"
        ),
        other => {
            panic!("a flooding producer must be stopped by the output allowance, not by {other:?}")
        }
    }
    assert!(
        elapsed < GENEROUS,
        "the allowance ended the wait, not the deadline: {elapsed:?}"
    );
}

#[test]
fn a_dead_producer_ends_the_wait_without_spending_the_bound() {
    const BOUND: Duration = Duration::from_secs(300);
    let scratch = Scratch::new("dead");
    let signal = scratch.join("signal");

    let mut producer = readiness_producer("dead", Some(&signal), Stdio::null());
    let started = Instant::now();
    let waited = readiness::await_signal(&signal, producer.child(), BOUND);
    let elapsed = started.elapsed();
    match waited {
        readiness::Waited::ProducerGone(why) => assert!(
            why.contains("without publishing"),
            "the report must name the death, not the clock: {why}"
        ),
        other => panic!("a producer that exited without publishing is not {other:?}"),
    }
    assert!(
        elapsed < BOUND,
        "the wait spent its whole bound: {elapsed:?}"
    );
    drop(producer);

    let mut producer = readiness_producer("dead", None, Stdio::piped());
    let started = Instant::now();
    let waited = producer.await_line("held", BOUND);
    let elapsed = started.elapsed();
    match waited {
        readiness::Waited::ProducerGone(why) => assert!(
            why.contains("closed its channel"),
            "a closed channel is the pipe's own fast path: {why}"
        ),
        other => panic!("a producer that closed its channel is not {other:?}"),
    }
    assert!(
        elapsed < BOUND,
        "the wait spent its whole bound: {elapsed:?}"
    );
}

#[test]
fn the_bound_is_the_callers_and_it_does_not_time_a_healthy_producer() {
    let scratch = Scratch::new("deadline");

    let silent = scratch.join("never");
    let mut producer = readiness_producer("silent", Some(&silent), Stdio::null());
    let mut spent = Vec::new();
    for bound in [Duration::from_millis(120), Duration::from_millis(480)] {
        let started = Instant::now();
        match readiness::await_signal(&silent, producer.child(), bound) {
            readiness::Waited::TimedOut(reported) => assert_eq!(reported, bound),
            other => panic!("a silent producer must time the wait out, not give {other:?}"),
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed >= bound,
            "ended before its own bound: {elapsed:?} < {bound:?}"
        );
        spent.push(elapsed);
    }
    assert!(
        spent[1] > spent[0],
        "the wait spends the bound it was given, not one of its own: {spent:?}"
    );
    drop(producer);

    const GENEROUS: Duration = Duration::from_secs(30);
    let signal = scratch.join("eventually");
    let mut producer = readiness_producer("signal-after", Some(&signal), Stdio::null());
    let started = Instant::now();
    let waited = readiness::await_signal(&signal, producer.child(), GENEROUS);
    let elapsed = started.elapsed();
    assert_eq!(
        waited.or_fail("the slow producer published and was still refused"),
        ["published"]
    );
    assert!(
        elapsed < GENEROUS,
        "the wait returned when the signal landed, not at its bound: {elapsed:?}"
    );
    drop(producer);

    let mut producer = readiness_producer("line-after", None, Stdio::piped());
    let started = Instant::now();
    let waited = producer.await_line("held", GENEROUS);
    let elapsed = started.elapsed();
    assert_eq!(
        waited.or_fail("the slow producer framed its line and was still refused"),
        ["held"]
    );
    assert!(
        elapsed < GENEROUS,
        "the wait returned when the line landed, not at its bound: {elapsed:?}"
    );
}

#[test]
fn a_signal_is_visible_only_after_the_state_it_announces() {
    let scratch = Scratch::new("ordering");
    let payload = "x".repeat(64 * 1024);

    let unsound = scratch.join("in-place");
    let observed = at_the_uncommitted_instant(&unsound, &payload, |signal, content| {
        let mut file = std::fs::File::create(signal).expect("create the signal in place");
        handshake();
        file.write_all(content.as_bytes()).expect("fill it after");
    });
    assert!(
        observed.existed,
        "the control is sharp only if the unsound form's name is observable before its \
         content: it was not, so the sound half below proves nothing"
    );
    assert_ne!(
        observed.content.as_deref(),
        Some(payload.as_str()),
        "the name existed and already carried the whole payload, so this control never \
         modelled the split it exists to model"
    );

    let sound = scratch.join("published");
    let observed = at_the_uncommitted_instant(&sound, &payload, |signal, content| {
        readiness::publish_between(signal, &[content], &mut || handshake()).expect("publish");
    });
    assert!(
        !observed.existed,
        "the signal name existed while its publication was still uncommitted, which is the \
         whole of what atomic publication rules out"
    );

    assert_eq!(
        readiness::read_published(&sound).expect("the published record reads"),
        [payload.as_str()]
    );
}

struct Observation {
    existed: bool,
    content: Option<String>,
}

struct Handshake {
    reached: mpsc::Sender<()>,
    resume: mpsc::Receiver<()>,
}

impl Handshake {
    fn hand_over(&self) {
        self.reached.send(()).expect("hand the observer its turn");
        self.resume.recv().expect("wait for the turn back");
    }
}

thread_local! {
    static HANDSHAKE: std::cell::RefCell<Option<Handshake>> =
        const { std::cell::RefCell::new(None) };
}

fn handshake() {
    HANDSHAKE.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("a producer runs inside an arranged instant")
            .hand_over();
    });
}

fn at_the_uncommitted_instant(
    signal: &Path,
    payload: &str,
    produce: fn(&Path, &str),
) -> Observation {
    let (reached_tx, reached_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let producing = signal.to_path_buf();
    let content = payload.to_owned();
    let producer = thread::spawn(move || {
        HANDSHAKE.with(|slot| {
            *slot.borrow_mut() = Some(Handshake {
                reached: reached_tx,
                resume: resume_rx,
            });
        });
        produce(&producing, &content);
    });

    reached_rx
        .recv()
        .expect("the producer reaches its uncommitted instant");
    let observation = Observation {
        existed: signal.exists(),
        content: std::fs::read_to_string(signal).ok(),
    };
    resume_tx.send(()).expect("give the producer its turn back");
    producer.join().expect("the producer thread");
    observation
}

#[test]
fn a_publication_that_fails_after_staging_removes_what_it_made() {
    let scratch = Scratch::new("cleanup");
    let signal = scratch.join("signal");

    readiness::publish(&signal, &["field"]).expect("publish");
    assert!(signal.exists(), "the signal is published");
    assert_eq!(staging_residue(&scratch), 0, "the rename spends the stage");

    let refused = scratch.join("refused");
    readiness::publish(&refused, &["two\nfields"]).expect_err("refused");
    assert!(
        !refused.exists(),
        "a refused publish must not create the name: an empty signal is still a claim"
    );
    assert_eq!(staging_residue(&scratch), 0, "nor a staging file");

    let blocked = scratch.join("blocked");
    let mut seam = || {
        assert_eq!(
            staging_residue(&scratch),
            1,
            "the record is staged at the seam, so the cleanup below has something to do"
        );
        std::fs::create_dir(&blocked).expect("block the rename");
    };
    let error = readiness::publish_between(&blocked, &["field"], &mut seam)
        .expect_err("a rename onto a directory cannot succeed");
    assert!(
        blocked.is_dir(),
        "the publication did not overwrite what blocked it: {error}"
    );
    assert_eq!(
        staging_residue(&scratch),
        0,
        "the staging file the failed publication created was removed by it"
    );

    let marker = scratch.join("marker");
    readiness::publish_marker(&marker).expect("publish a marker");
    assert!(
        readiness::read_published(&marker)
            .expect("a marker reads")
            .is_empty(),
        "a marker announces state it has nothing to say about"
    );

    readiness::publish(&signal, &["second"]).expect("republish");
    assert_eq!(
        readiness::read_published(&signal).expect("read back"),
        ["second"]
    );
    assert_eq!(staging_residue(&scratch), 0, "and leaves nothing behind");
}

fn staging_residue(dir: &Path) -> usize {
    staging_names(dir).len()
}

#[test]
fn concurrent_publications_do_not_share_a_staging_name() {
    const PUBLISHERS: usize = 8;
    let scratch = Scratch::new("staging");
    let signal = scratch.join("contended");
    let all_staged = std::sync::Barrier::new(PUBLISHERS);

    let seen: Vec<Vec<String>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..PUBLISHERS)
            .map(|which| {
                let signal = &signal;
                let root: &Path = &scratch;
                let all_staged = &all_staged;
                scope.spawn(move || {
                    let mut staged_now = Vec::new();
                    let mut seam = || {
                        all_staged.wait();
                        staged_now = staging_names(root);
                        all_staged.wait();
                    };
                    readiness::publish_between(signal, &[&format!("publisher-{which}")], &mut seam)
                        .expect("publish");
                    staged_now
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a publisher"))
            .collect()
    });

    for (which, staged_now) in seen.iter().enumerate() {
        assert_eq!(
            staged_now.len(),
            PUBLISHERS,
            "publisher {which} saw {} staged record(s) where all {PUBLISHERS} were staged: \
             {staged_now:?}",
            staged_now.len()
        );
    }
    let distinct: std::collections::BTreeSet<&str> =
        seen.iter().flatten().map(String::as_str).collect();
    assert_eq!(
        distinct.len(),
        PUBLISHERS,
        "{PUBLISHERS} concurrent publications must hold {PUBLISHERS} staging names, not \
         {}: a shared name is one file two publishers are writing: {distinct:?}",
        distinct.len()
    );
    assert_eq!(
        staging_residue(&scratch),
        0,
        "and every publication spent its own stage"
    );
    let published = readiness::read_published(&signal).expect("a whole record survives");
    let [only] = published.as_slice() else {
        panic!("the contended signal carries one field, not {published:?}")
    };
    assert!(
        (0..PUBLISHERS).any(|which| only == &format!("publisher-{which}")),
        "the surviving record is not one anybody published: {only}"
    );
}

fn staging_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("the scratch directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".publishing"))
        .collect()
}
