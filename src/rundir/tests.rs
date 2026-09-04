// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/rundir.rs` -- the shape `src/runner/container/tests.rs`
// established for a funnel's own test module. This suite plants husks and
// residue with raw `fs` calls, forks and locks real descriptors, and re-execs
// this test binary, so it names those primitives directly.
//
// `PR6-LANEF-004`: a Rust lint level is scoped by the MODULE TREE and not by
// the file, so without an attribute here the parent's inner allow would reach
// this file silently and no reviewed record would name the file doing the work.
// All three are needed and all three are measured; none is inherited.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use super::*;
// The split moved the classification probe into a child. `use super::*` reaches the
// parent's namespace only, so the five items this suite drives directly --
// `FIRST_LINE_WINDOW`, `SCAN_CHUNK`, `first_line`, `first_line_within` and
// `RunStartedHeader` -- are reached through the child's own `pub(super)` surface, and
// the three `std::io` traits it drove them with are named here rather than borrowed
// from the parent's import list, which no longer needs them. No test is renamed, no
// assertion changes and no body moves; these two lines are the whole of what the
// extraction owes this file.
use super::classify::*;

use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

use crate::agent::proc::test_support::readiness;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("upstroke-rundir-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn paths_in(root: &Path, run_id: &str) -> RunPaths {
    RunPaths::with_private_root(&root.join("repo"), run_id, &root.join("home"))
}

/// The exact bytes of a committed first line, written by hand.
///
/// Not `serde_json::to_string(&Event::now(…))`: the classifier is judged
/// against the **wire**, and a fixture that serialized through the same
/// types the classifier reads would agree with any symmetric change to
/// both (`PR3-WIRE-PINNING`). Every field here is one the packet names —
/// the `event` tag, and `schema` and `run_id` inside `data`, which is what
/// recovery step (a0) means by "probe the header of the committed first
/// line … select the engine by schema".
fn committed_line(run_id: &str, schema: u32) -> String {
    format!(
        "{{\"ts\":\"2026-08-20T00:00:00Z\",\"event\":\"run_started\",\
             \"data\":{{\"schema\":{schema},\"run_id\":\"{run_id}\",\
             \"branch\":\"upstroke/run-{run_id}\"}}}}"
    )
}

/// Make `<repo>/.upstroke/runs/<run_id>` a committed run.
fn commit_run(repo: &Path, run_id: &str) -> PathBuf {
    let public = public_dir(repo, run_id);
    fs::create_dir_all(&public).expect("run dir");
    fs::write(
        public.join(EVENT_LOG),
        format!("{}\n", committed_line(run_id, 3)),
    )
    .expect("committed first line");
    public
}

#[test]
fn agent_authored_files_land_outside_the_workspace() {
    // The whole point of the split: a reviewer with read access to the
    // repo has no path to the implementer's transcript.
    let root = scratch("split");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");

    let repo = root.join("repo");
    for private in [
        paths.transcripts(),
        paths.reviews(),
        paths.settings(),
        paths.gates(),
        paths.gate_worktrees(),
    ] {
        assert!(private.is_dir(), "{} should exist", private.display());
        assert!(
            !private.starts_with(&repo),
            "{} must not be inside the workspace",
            private.display()
        );
    }
    for public in [paths.questions(), paths.answers(), paths.artifacts()] {
        assert!(
            public.starts_with(&repo),
            "ops surface stays beside the repo"
        );
    }
    assert_eq!(
        paths.events(),
        repo.join(".upstroke/runs/RUN1/events.jsonl")
    );
}

#[test]
fn the_private_fallback_is_never_the_workspace() {
    // No HOME is a bad day, not a reason to quietly put transcripts back
    // where an agent can read them.
    let root = default_private_root();
    assert!(
        root.ends_with(".upstroke") || root.ends_with("upstroke"),
        "{root:?}"
    );
    assert!(root.is_absolute(), "{root:?}");
}

#[test]
fn runs_list_chronologically_and_resolve_by_prefix() {
    let root = scratch("discover");
    let repo = root.join("repo");
    for id in ["01AAA", "01BBB", "01BCC"] {
        commit_run(&repo, id);
    }
    assert_eq!(list_runs(&repo), ["01AAA", "01BBB", "01BCC"]);
    assert_eq!(latest_run(&repo).as_deref(), Some("01BCC"));

    assert_eq!(resolve_run_id(&repo, "01AAA").expect("exact"), "01AAA");
    assert_eq!(resolve_run_id(&repo, "01A").expect("prefix"), "01AAA");
    assert_eq!(
        resolve_run_id(&repo, "01bcc").expect("case-insensitive"),
        "01BCC"
    );

    let err = resolve_run_id(&repo, "01B").expect_err("ambiguous");
    assert!(err.to_string().contains("matches 2 runs"), "got: {err}");
    let err = resolve_run_id(&repo, "02").expect_err("no match");
    assert!(err.to_string().contains("known runs"), "got: {err}");
}

#[test]
fn an_empty_repo_names_where_it_looked() {
    let root = scratch("norun");
    let err = resolve_run_id(&root.join("repo"), "01A").expect_err("nothing to resume");
    assert!(err.to_string().contains("no runs found"), "got: {err}");
}

#[test]
fn questions_resolve_to_their_run_by_prefix() {
    let root = scratch("questions");
    let repo = root.join("repo");
    for (run, question) in [
        ("01AAA", "q-ONE"),
        ("01BBB", "q-TWO"),
        ("01BBB", "q-TWENTY"),
    ] {
        let dir = commit_run(&repo, run).join("questions");
        fs::create_dir_all(&dir).expect("questions dir");
        fs::write(dir.join(format!("{question}.json")), "{}").expect("question");
    }

    let found = find_question(&repo, "q-ONE").expect("exact");
    assert_eq!(found.run_id, "01AAA");
    assert_eq!(found.question_id, "q-ONE");
    assert_eq!(found.public, public_dir(&repo, "01AAA"));

    // A full id wins even though `q-TWO` is also a prefix of `q-TWENTY`.
    let found = find_question(&repo, "q-TWO").expect("exact beats prefix");
    assert_eq!(found.question_id, "q-TWO");
    assert_eq!(found.run_id, "01BBB");

    let err = find_question(&repo, "q-TW").expect_err("ambiguous");
    assert!(
        err.to_string().contains("matches 2 questions"),
        "got: {err}"
    );
    let err = find_question(&repo, "q-NONE").expect_err("no match");
    assert!(err.to_string().contains("no question"), "got: {err}");
}

#[test]
fn a_run_can_only_be_held_once_at_a_time() {
    let root = scratch("lock");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");

    assert!(
        !is_running(&paths.public),
        "nothing holds a run that never started"
    );
    let held = RunLock::acquire(&paths.public).expect("first acquire");
    assert!(is_running(&paths.public), "status can see the run is live");

    // This one is `claims`, not the OS: `fcntl` locks belong to the process,
    // so both of these would succeed if the file were the only guard.
    // Cross-process exclusion — the property that actually matters — is
    // `a_second_process_is_refused_the_run_lock` below.
    let err = RunLock::acquire(&paths.public).expect_err("a second engine is refused");
    assert!(
        err.to_string().contains("already driving run"),
        "got: {err}"
    );

    // A refusal that failed still leaves the run exactly as claimed as it
    // was — a bookkeeping slip here would either free a live run or strand
    // a dead one.
    assert!(
        is_running(&paths.public),
        "the failed acquire changed nothing"
    );

    // Dropping releases it — which is also what a crash does, so resume
    // never has to clear a stale marker by hand.
    drop(held);
    assert!(!is_running(&paths.public));
    RunLock::acquire(&paths.public).expect("re-acquire after release");
}

#[cfg(unix)]
#[test]
fn same_process_handoff_closes_old_descriptor_before_publishing_claim_free() {
    let root = scratch("orderedhandoff");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");
    let mut held = RunLock::acquire(&paths.public).expect("first acquire");

    held.release_file_then(|| {
        let file = File::open(lock_file(&paths.public)).expect("inspect released lock");
        assert!(
            matches!(imp::holder(&file), Holder::Nobody),
            "the old descriptor must already be closed"
        );
        let error = RunLock::acquire(&paths.public)
            .expect_err("the in-process claim stays published until after close");
        assert!(error.to_string().contains("already driving run"), "{error}");
    });

    let replacement = RunLock::acquire(&paths.public).expect("handoff after ordered release");
    drop(replacement);
}

#[cfg(unix)]
#[test]
fn cleanup_lease_failure_closes_primary_before_releasing_claim() {
    let root = scratch("cleanupfailurehandoff");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");
    let mut held = RunLock::acquire(&paths.public).expect("primary acquired");
    let file = held._file.take();
    let claim = held.claim.clone();

    // This is the exact rollback primitive used when cleanup::take fails.
    // The callback is a deterministic observation point between closing
    // the POSIX descriptor and publishing the same-process claim as free.
    release_claim_after_file(file, &claim, || {
        let file = File::open(lock_file(&paths.public)).expect("inspect primary lock");
        assert!(matches!(imp::holder(&file), Holder::Nobody));
        RunLock::acquire(&paths.public)
            .expect_err("claim cannot be reused until the old descriptor is closed");
    });

    let replacement = RunLock::acquire(&paths.public).expect("clean rollback handoff");
    drop(replacement);
    drop(held);
}

#[test]
fn a_run_lock_remains_send_even_though_its_cleanup_scope_is_thread_local() {
    fn assert_send<T: Send>() {}
    assert_send::<RunLock>();
}

#[test]
fn the_lock_answers_at_once_rather_than_waiting_to_be_sure() {
    // There was a 500ms contention grace here, and it was paid in full
    // exactly when the answer was yes: a live engine never lets go, so the
    // retry loop always ran to the deadline. Every `upstroke status` and
    // `upstroke answer` against a working run paid it, and `--follow` paid it
    // once per idle poll until it was given a cheaper question to ask.
    //
    // The grace existed to disbelieve a `fork` window. The primitive now
    // rules that out outright, so there is nothing left to wait for.
    let root = scratch("prompt");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");
    let _held = RunLock::acquire(&paths.public).expect("acquire");

    let started = Instant::now();
    for _ in 0..20 {
        assert!(is_running(&paths.public));
    }
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_millis(100),
        "twenty probes of a live run took {waited:?} — something is waiting again"
    );
}

/// A `fork` that has not reached its `exec` yet, held open on purpose.
///
/// The child does nothing but sleep and `_exit`, both of which are safe in
/// the child of a threaded process — no allocation, no locks, no
/// destructors.
#[cfg(unix)]
fn fork_a_sleeper(ms: u64) -> libc::pid_t {
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        std::thread::sleep(Duration::from_millis(ms));
        unsafe { libc::_exit(0) };
    }
    assert!(pid > 0, "fork failed");
    pid
}

#[cfg(unix)]
#[test]
fn a_fork_cannot_keep_a_released_run_locked() {
    // The bug the whole design turns on, deterministically.
    //
    // `flock` belongs to the open file description, and `fork` duplicates
    // every descriptor — so a child holds the run's lock until it execs,
    // and an engine that has finished and let go still reads as live for
    // that whole window. It was measured at 50 false positives in 3000
    // probes under a suite that spawns subprocesses, and each one made a
    // run refuse to start against an engine that did not exist, or a
    // finished run report itself as running.
    //
    // Against `flock` this test fails outright: the probe below sees the
    // lock held by the sleeping child. `fcntl` locks are not inherited, so
    // releasing really releases.
    let root = scratch("forkwindow");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");

    let held = RunLock::acquire(&paths.public).expect("acquire");
    let sleeper = fork_a_sleeper(400);
    // The engine finishes while that child is still between fork and exec.
    drop(held);

    assert!(
        !is_running(&paths.public),
        "a forked child was still holding the run's lock"
    );
    RunLock::acquire(&paths.public).expect("and a second engine can start");

    let mut status = 0;
    unsafe { libc::waitpid(sleeper, &mut status, 0) };
}

/// The child half of `a_second_process_is_refused_the_run_lock`: takes the
/// lock, says so, and holds it until it is killed.
///
/// An `#[ignore]`d test re-invoked as a subprocess, which is how
/// `killing_a_run_mid_attempt_leaves_a_resumable_record` gets a real second
/// process too.
#[test]
#[ignore = "spawned as a subprocess by a_second_process_is_refused_the_run_lock"]
fn lock_child_holds_the_run() {
    let public = PathBuf::from(std::env::var("UPSTROKE_TEST_LOCK_DIR").expect("run dir"));
    let _held = RunLock::acquire(&public).expect("the child takes the lock");
    println!("held");
    std::io::Write::flush(&mut std::io::stdout()).expect("flush");
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
#[ignore = "spawned as a subprocess by two_run_ids_cannot_drive_one_worktree_concurrently"]
fn worktree_lock_child_holds_run_a() {
    let repo = PathBuf::from(std::env::var("UPSTROKE_TEST_WORKTREE_DIR").expect("repo"));
    let git_dir = PathBuf::from(std::env::var("UPSTROKE_TEST_WORKTREE_GIT_DIR").expect("git dir"));
    let public = PathBuf::from(std::env::var("UPSTROKE_TEST_LOCK_DIR").expect("run dir"));
    let _worktree = WorktreeLock::acquire_in(&repo, &git_dir).expect("child takes worktree lease");
    let _run = RunLock::acquire(&public).expect("child takes run A lock");
    println!("held");
    std::io::Write::flush(&mut std::io::stdout()).expect("flush");
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn two_run_ids_cannot_drive_one_worktree_concurrently() {
    let root = scratch("two-runs-one-worktree");
    let repo = root.join("repo");
    let git_dir = root.join("git-dir");
    fs::create_dir_all(&git_dir).expect("worktree git dir");
    let run_a = paths_in(&repo, "RUNA");
    let run_b = paths_in(&repo, "RUNB");
    run_a.create().expect("run A dirs");
    run_b.create().expect("run B dirs");

    let exe = std::env::current_exe().expect("test binary");
    // Adopted, so the child is terminated, reaped and its reader joined
    // when this scope ends however it ends -- including a panicking
    // assertion between here and the teardown below.
    let mut producer = readiness::Producer::adopt(
        std::process::Command::new(exe)
            .args([
                "--exact",
                "rundir::tests::worktree_lock_child_holds_run_a",
                "--ignored",
                "--nocapture",
            ])
            .env("UPSTROKE_TEST_WORKTREE_DIR", &repo)
            .env("UPSTROKE_TEST_WORKTREE_GIT_DIR", &git_dir)
            .env("UPSTROKE_TEST_LOCK_DIR", &run_a.public)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn run A engine"),
    );

    // Producer-aware and *effectively* bounded, at the bound this test
    // already used. The loop it replaces checked its deadline only after
    // `read_line` returned, so against a producer that stayed alive and
    // silent -- the one case CODING_STANDARDS.md §12 says the bound exists
    // for -- the read blocked and the deadline was never reached at all.
    producer
        .await_line("held", Duration::from_secs(30))
        .or_fail("run A child never took its leases");

    // The per-run lock alone would allow this: the identifiers and files
    // differ. The outer lease is what owns shared HEAD/index/worktree state.
    let run_b_only = RunLock::acquire(&run_b.public).expect("run B lock is independent");
    drop(run_b_only);
    let error =
        WorktreeLock::acquire_in(&repo, &git_dir).expect_err("run B must lose the worktree lease");
    assert!(
        error.to_string().contains("already driving worktree"),
        "{error}"
    );

    drop(producer);
}

#[test]
fn a_second_process_is_refused_the_run_lock() {
    // The property `claims` cannot provide and the file lock exists for.
    // Two engines are two processes, and `fcntl` locks are per-process —
    // which is exactly why this has to be tested across a real process
    // boundary rather than against a second `acquire` here.
    let root = scratch("twoprocs");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");

    let exe = std::env::current_exe().expect("test binary");
    // Adopted, so the child is terminated, reaped and its reader joined
    // when this scope ends however it ends -- including a panicking
    // assertion between here and the teardown below.
    let mut producer = readiness::Producer::adopt(
        std::process::Command::new(exe)
            .args([
                "--exact",
                "rundir::tests::lock_child_holds_the_run",
                "--ignored",
                "--nocapture",
            ])
            .env("UPSTROKE_TEST_LOCK_DIR", &paths.public)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn the second engine"),
    );

    // Wait for it to say it has the lock, rather than sleeping and hoping.
    // Producer-aware and effectively bounded, at the bound this test
    // already used; see `two_run_ids_cannot_drive_one_worktree_concurrently`
    // for what the loop this replaces could not do.
    producer
        .await_line("held", Duration::from_secs(30))
        .or_fail("the child never took the lock");

    let err = RunLock::acquire(&paths.public).expect_err("a second engine must be refused");
    assert!(
        err.to_string().contains("already driving run"),
        "got: {err}"
    );
    assert!(is_running(&paths.public), "and status agrees it is live");

    // `F_GETLK` names the holder, so the refusal can say who instead of
    // leaving the operator to find it. Asserted here rather than against a
    // second `acquire` in this process, because that one is refused by
    // `claims`, which knows this pid without asking the OS anything — it
    // would pass whatever the lock did.
    #[cfg(unix)]
    assert!(
        err.to_string()
            .contains(&format!("pid {}", producer.child().id())),
        "the refusal should name the process actually holding it: {err}"
    );

    drop(producer);
}

#[cfg(unix)]
#[test]
fn a_holder_never_opens_its_own_lock_file() {
    // `fcntl`'s sharpest edge: closing *any* descriptor for a file releases
    // every lock this process holds on it. So a holder that does what
    // `is_running` does — open the lock file, look, drop it — hands the run
    // away silently, and the next `acquire` anywhere succeeds against a
    // live engine.
    //
    // `is_running` answers from `claims` before it would open anything,
    // which is what makes that unreachable. This test is here because the
    // rule is invisible in the code that depends on it.
    let root = scratch("selfclose");
    let paths = paths_in(&root, "RUN1");
    paths.create().expect("create");
    let _held = RunLock::acquire(&paths.public).expect("acquire");

    // The call a holder is most likely to make.
    assert!(is_running(&paths.public));

    // If that had gone to the file, the lock would be gone by now — ask
    // from a process that has no claim of its own to answer from.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        let file = File::open(lock_file(&paths.public)).expect("open");
        let free = matches!(imp::holder(&file), Holder::Nobody);
        unsafe { libc::_exit(i32::from(free)) };
    }
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(
        libc::WEXITSTATUS(status),
        0,
        "the holder released its own lock by looking at it"
    );
}

#[test]
fn a_lock_the_os_will_not_report_on_is_not_a_free_lock() {
    // No filesystem CI runs on returns `ENOLCK`, so the decision is checked
    // where it is made. A lock the OS declines to report on must not come
    // back as "nobody is running", because that is the reading that tells
    // an operator to resume a run that is still in flight.
    let unknown = Holder::Unknown(io::Error::from_raw_os_error(ENOLCK_LIKE));
    assert!(
        !matches!(unknown, Holder::Nobody),
        "an error is not an answer"
    );
}

/// Any errno at all; the value is not what is under test.
const ENOLCK_LIKE: i32 = 37;

#[test]
fn an_exact_match_resolves_to_the_name_on_disk() {
    // The comparison is case-insensitive, so the answer has to be the
    // directory that actually exists: on a case-sensitive filesystem the
    // uppercased input names nothing, and every caller joins this id onto
    // a path.
    let root = scratch("ondisk");
    let repo = root.join("repo");
    commit_run(&repo, "01AbCd");

    assert_eq!(resolve_run_id(&repo, "01abcd").expect("exact"), "01AbCd");
    assert_eq!(resolve_run_id(&repo, "01AB").expect("prefix"), "01AbCd");
}

// =======================================================================
// Classification
// =======================================================================

/// One directory shape, its construction, and the class the packet gives
/// it. The expected value is transcribed from the packet's own rule and
/// never computed by the function under test.
struct DirShape {
    name: &'static str,
    build: fn(&Path),
    expected: RunDirClass,
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    fs::write(path, bytes).expect("write");
}

/// A marker that **names the private half sitting beside it**.
///
/// `any_marker_bytes` records `/nowhere/runs/01SHAPE`, which is what let
/// `PR5-RUNDIR-005` and `PR5-RUNDIR-006` survive the thirteen-shape grid: a
/// classifier that *follows the locator* looked in `/nowhere`, found
/// nothing, fell through to the first-line probe and answered `Husk` for
/// the wrong reason. The grid proved the classifier ignores a private half
/// sitting beside it; it never proved the classifier ignores the private
/// half the marker actually names — and "`Committed` by a valid
/// newline-terminated first-line `run_started`, else `Husk`" is a claim
/// about every private half, named or not.
fn marker_bytes_locating(private: &Path) -> Vec<u8> {
    serde_json::to_vec(&CreatingMarker {
        run_id: "01SHAPE".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        private_dir: private.to_string_lossy().into_owned(),
        incarnation: "01INC".to_owned(),
        pid: 4242,
        runner_policy_sha256: "sha256:00".to_owned(),
    })
    .expect("marker json")
}

/// An owner record with every field populated, so a classifier that parses
/// what it finds is caught as surely as one that only stats it.
fn plausible_owner_bytes() -> Vec<u8> {
    serde_json::to_vec(&OwnerRecord {
        run_id: "01SHAPE".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        public_dir: "/nowhere/public".to_owned(),
        incarnation: "01INC".to_owned(),
        runner: crate::runner::policy::host_policy(),
    })
    .expect("owner json")
}

/// A marker whose fields do not matter to the classifier, which is the
/// point: `startup_census` classifies "whether or not a marker is present".
fn any_marker_bytes() -> Vec<u8> {
    serde_json::to_vec(&CreatingMarker {
        run_id: "01SHAPE".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        private_dir: "/nowhere/runs/01SHAPE".to_owned(),
        incarnation: "01INC".to_owned(),
        pid: 4242,
        runner_policy_sha256: "sha256:00".to_owned(),
    })
    .expect("marker json")
}

/// The publication prefixes P0–P8, as `classify_run_dir`'s proof test
/// names them.
///
/// The contract's list — "bare, staged-marker, marker-only, marker+lock,
/// marker+private (with and without owner record; with and without commit
/// record), log-without-committed-first-line, torn-first-line,
/// committed-with-marker, malformed-marker, and committed" — reads as a
/// crossing on the `marker+private` entry, so the maximal reading is
/// thirteen shapes and the collapsed one is ten. This table carries the
/// maximal reading plus the shapes `startup_census` names that the
/// contract's phrase does not spell out separately, because covering
/// thirteen covers twelve whichever way the sentence is read.
fn shapes() -> Vec<DirShape> {
    vec![
        DirShape {
            name: "bare",
            build: |public| fs::create_dir_all(public).expect("bare"),
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "staged-marker",
            build: |public| write(&public.join(MARKER_STAGED), &any_marker_bytes()),
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker-only",
            build: |public| write(&public.join(MARKER), &any_marker_bytes()),
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker+lock",
            build: |public| {
                write(&public.join(MARKER), &any_marker_bytes());
                write(&lock_file(public), b"");
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker+private-with-owner-record",
            build: |public| {
                write(&public.join(MARKER), &any_marker_bytes());
                write(&public.join("private/owner.json"), b"{}");
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker+private-without-owner-record",
            build: |public| {
                write(&public.join(MARKER), &any_marker_bytes());
                fs::create_dir_all(public.join("private")).expect("private");
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker+private-with-commit-record",
            build: |public| {
                write(&public.join(MARKER), &any_marker_bytes());
                write(&public.join("private/owner.json"), b"{}");
                write(&public.join("private/committed.json"), b"{}");
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker+private-without-commit-record",
            build: |public| {
                write(&public.join(MARKER), &any_marker_bytes());
                write(&public.join("private/owner.json"), b"{}");
                write(&public.join(PLAN), b"{}");
            },
            expected: RunDirClass::Husk,
        },
        // The two shapes the grid was missing: the marker names the
        // private half that is really there. A classifier that follows the
        // locator answers `Committed` for both, and only these two shapes
        // can tell it from one that does not.
        DirShape {
            name: "marker-bound-private-with-owner-record",
            build: |public| {
                let private = public.join("private");
                write(&private.join(OWNER_RECORD), &plausible_owner_bytes());
                write(&public.join(MARKER), &marker_bytes_locating(&private));
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "marker-bound-private-with-commit-record",
            build: |public| {
                let private = public.join("private");
                write(&private.join(OWNER_RECORD), &plausible_owner_bytes());
                write(
                    &private.join(COMMIT_RECORD),
                    b"{\"run_started_sha256\":\"sha256:00\"}",
                );
                write(&public.join(MARKER), &marker_bytes_locating(&private));
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "log-without-committed-first-line",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    b"{\"ts\":\"t\",\"event\":\"attempt_started\",\"data\":{}}\n",
                );
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "torn-first-line",
            build: |public| {
                // The newline is the commit marker, so a first line
                // without one is not an event and never was.
                let torn = committed_line("01TORN", 3);
                write(&public.join(EVENT_LOG), &torn.as_bytes()[..torn.len() - 8]);
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            // The shape above truncates the JSON as well as the newline, so
            // it refuses on the parse and stays green if the *terminator*
            // requirement is dropped — measured: a `first_committed_line`
            // that treats end-of-file as end-of-line survived the whole
            // grid. This shape isolates the terminator: a complete, valid,
            // parseable `run_started` whose only defect is that it was
            // never terminated. `startup_census` says "first
            // **newline-terminated** line", and the newline is the only
            // evidence that the writer finished writing it.
            name: "complete-first-line-with-no-newline",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    committed_line("01SHAPE", 3).as_bytes(),
                );
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "malformed-marker",
            build: |public| {
                write(&public.join(MARKER), b"{ not json");
                write(&public.join(PLAN), b"{}");
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "committed",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    format!("{}\n", committed_line("01SHAPE", 3)).as_bytes(),
                );
            },
            expected: RunDirClass::Committed,
        },
        DirShape {
            name: "committed-with-marker",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    format!("{}\n", committed_line("01SHAPE", 3)).as_bytes(),
                );
                write(&public.join(MARKER), &any_marker_bytes());
            },
            expected: RunDirClass::Committed,
        },
        // Beyond the contract's list, from `startup_census`'s own
        // enumeration and from the rule's own edges.
        DirShape {
            name: "committed-with-staged-marker",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    format!("{}\n", committed_line("01SHAPE", 3)).as_bytes(),
                );
                write(&public.join(MARKER_STAGED), &any_marker_bytes());
            },
            expected: RunDirClass::Committed,
        },
        DirShape {
            name: "empty-log",
            build: |public| write(&public.join(EVENT_LOG), b""),
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "blank-first-line-then-run-started",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    format!("\n{}\n", committed_line("01SHAPE", 3)).as_bytes(),
                );
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "first-line-is-not-json",
            build: |public| write(&public.join(EVENT_LOG), b"not json at all\n"),
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "first-line-has-no-schema-to-select-by",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    b"{\"event\":\"run_started\",\"data\":{\"run_id\":\"01SHAPE\"}}\n",
                );
            },
            expected: RunDirClass::Husk,
        },
        DirShape {
            name: "committed-first-line-with-a-torn-tail",
            build: |public| {
                // A torn *tail* is truncated by the next open and was
                // never an event; it says nothing about the first line.
                write(
                    &public.join(EVENT_LOG),
                    format!(
                        "{}\n{{\"ts\":\"t\",\"event\":\"attempt_star",
                        committed_line("01SHAPE", 3)
                    )
                    .as_bytes(),
                );
            },
            expected: RunDirClass::Committed,
        },
        DirShape {
            name: "committed-schema-4",
            build: |public| {
                write(
                    &public.join(EVENT_LOG),
                    format!("{}\n", committed_line("01SHAPE", 4)).as_bytes(),
                );
            },
            expected: RunDirClass::Committed,
        },
    ]
}

#[test]
fn every_publication_prefix_classifies_as_the_packet_names_it() {
    let root = scratch("shapes");
    let mut committed = 0usize;
    let mut husks = 0usize;
    for shape in shapes() {
        let public = root.join(shape.name);
        fs::create_dir_all(&public).expect("shape dir");
        (shape.build)(&public);
        let actual = classify_run_dir(&public);
        assert_eq!(
            actual, shape.expected,
            "shape `{}` classified {actual:?}",
            shape.name
        );
        match shape.expected {
            RunDirClass::Committed => committed += 1,
            RunDirClass::Husk => husks += 1,
        }
    }
    // Distinct-value counts rather than prose: a grid that had drifted to
    // one class would still pass every assertion above.
    assert_eq!(committed, 5, "committed shapes");
    assert_eq!(husks, 18, "husk shapes");
    // The two marker-bound shapes are the ones a locator-following
    // classifier gets wrong, so their presence is asserted rather than
    // left to the count above.
    let names: Vec<&str> = shapes().iter().map(|shape| shape.name).collect();
    for bound in [
        "marker-bound-private-with-owner-record",
        "marker-bound-private-with-commit-record",
    ] {
        assert!(names.contains(&bound), "the grid lost `{bound}`");
    }
    assert!(
        committed + husks >= 13,
        "the contract's list reads as thirteen shapes at its widest"
    );
}

#[test]
fn a_missing_directory_and_a_missing_log_are_both_husks() {
    let root = scratch("absent");
    assert_eq!(classify_run_dir(&root.join("nothing")), RunDirClass::Husk);
    let bare = root.join("bare");
    fs::create_dir_all(&bare).expect("bare");
    assert_eq!(classify_run_dir(&bare), RunDirClass::Husk);
}

/// A valid `run_started` line, terminated, whose total length is exactly
/// `total` bytes.
///
/// The padding is a field *inside* the object, so the line stays a valid
/// `run_started` at every length — a fixture that padded outside the JSON
/// would refuse on the parse and could never distinguish a length bound
/// from a parse failure. That confound is the `bounded_grid` shape recorded
/// four times in `reviews/FINDINGS.md`, and `PR5B-CLASSIFIER-TERMINATOR-
/// UNTESTED` is the same file's most recent instance.
fn committed_line_of_exactly(run_id: &str, total: usize) -> Vec<u8> {
    let line = committed_line(run_id, 3);
    let head = &line[..line.len() - 1];
    let overhead = head.len() + ",\"pad\":\"".len() + "\"}".len() + "\n".len();
    assert!(
        total >= overhead,
        "a {total}-byte line cannot hold a run_started at all"
    );
    let padded = format!("{head},\"pad\":\"{}\"}}\n", "x".repeat(total - overhead));
    assert_eq!(padded.len(), total, "the padding arithmetic is off");
    padded.into_bytes()
}

/// `FIRST_LINE_WINDOW` decides how many syscalls the probe makes, and
/// nothing about what a directory *is*.
///
/// `startup_census` defines `Committed` as "`events.jsonl` exists and its
/// first **newline-terminated** line is a valid `run_started`" and states no
/// size exception, so every length classifies the same way. Six lengths
/// straddling the window in both directions, including a line four times
/// the window — which is `PR5-CORRECTNESS-002`'s failure sequence at
/// `FIRST_LINE_WINDOW + 1` and three orders of magnitude past it.
///
/// The lengths are written relative to the constant on purpose: the claim
/// is *independence*, so shrinking the constant must leave this test
/// passing. What would fail is any re-introduction of a length bound —
/// which is the mutation that matters here, and it is witnessed in
/// `reviews/FINDINGS.md`.
#[test]
fn classification_does_not_depend_on_the_probe_window() {
    let root = scratch("window");
    let window = usize::try_from(FIRST_LINE_WINDOW).expect("the window fits a usize");
    let mut lengths = std::collections::BTreeSet::new();
    for (label, total) in [
        ("tiny", 512),
        ("just under a chunk", SCAN_CHUNK - 1),
        ("exactly a chunk", SCAN_CHUNK),
        ("just under the window", window - 1),
        ("exactly the window", window),
        ("one past the window", window + 1),
        ("four windows", window * 4),
    ] {
        lengths.insert(total);
        let public = root.join(label.replace(' ', "-"));
        write(
            &public.join(EVENT_LOG),
            &committed_line_of_exactly("01WINDOW", total),
        );
        assert_eq!(
            classify_run_dir(&public),
            RunDirClass::Committed,
            "a {total}-byte valid run_started line ({label}) is committed at every length"
        );
    }
    assert_eq!(lengths.len(), 7, "seven distinct lengths: {lengths:?}");
    assert!(
        lengths.iter().filter(|len| **len > window).count() >= 2,
        "at least two lengths past the window, or the claim is untested: {lengths:?}"
    );
}

/// The terminator is still the whole of the difference, at every length.
///
/// `PR5B-CLASSIFIER-TERMINATOR-UNTESTED` added the un-terminated shape at
/// one small length; the fall-back path this slice added is a *second*
/// implementation of "is there a newline", so it gets the same question.
/// The two files differ in exactly one byte's presence.
#[test]
fn a_complete_first_line_with_no_terminator_is_a_husk_at_every_length() {
    let root = scratch("unterminated");
    let window = usize::try_from(FIRST_LINE_WINDOW).expect("the window fits a usize");
    for (label, total) in [
        ("inside the window", 4096),
        ("past the window", window + 4096),
    ] {
        let terminated = committed_line_of_exactly("01TERM", total);
        let unterminated = &terminated[..terminated.len() - 1];
        assert_eq!(
            terminated.last(),
            Some(&b'\n'),
            "{label}: the fixture must be terminated"
        );
        assert!(
            !unterminated.contains(&b'\n'),
            "{label}: dropping the terminator must leave no newline at all"
        );

        let committed = root.join(format!("{}-terminated", label.replace(' ', "-")));
        write(&committed.join(EVENT_LOG), &terminated);
        assert_eq!(
            classify_run_dir(&committed),
            RunDirClass::Committed,
            "{label}: the terminated fixture"
        );

        let husk = root.join(format!("{}-torn", label.replace(' ', "-")));
        write(&husk.join(EVENT_LOG), unterminated);
        assert_eq!(
            classify_run_dir(&husk),
            RunDirClass::Husk,
            "{label}: the same bytes without the terminator"
        );
    }
}

/// The line the probe hands to the parser is the line, exactly.
///
/// An off-by-one in the fall-back's newline offset is the defect the new
/// code could carry: one byte short truncates the closing brace and one
/// byte long splices the newline into the JSON, and *both* refuse on the
/// parse — so `Husk` would look like a correct answer for the wrong reason.
/// This asserts the bytes rather than the verdict, on both paths.
#[test]
fn the_probe_returns_the_lines_exact_bytes_on_both_paths() {
    let root = scratch("exact");
    let window = usize::try_from(FIRST_LINE_WINDOW).expect("the window fits a usize");
    for (label, total) in [("window path", 4096), ("scan path", window + 7)] {
        let line = committed_line_of_exactly("01EXACT", total);
        let mut bytes = line.clone();
        // A second event after it, so "read to end of file" and "read to
        // the first newline" are different answers.
        bytes.extend_from_slice(b"{\"ts\":\"2026-08-20T00:00:01Z\",\"event\":\"noise\"}\n");
        let path = root.join(label.replace(' ', "-")).join(EVENT_LOG);
        write(&path, &bytes);

        let mut file = File::open(&path).expect("open");
        let read = first_line(&mut file).expect("a newline-terminated first line");
        assert_eq!(
            read,
            line[..line.len() - 1].to_vec(),
            "{label}: the probe returned {} bytes for a {}-byte line",
            read.len(),
            line.len() - 1
        );
    }
}

/// A source that never ends: every read hands back non-newline bytes and
/// it is never at end of file. `/dev/zero`, on a host that has one and on a
/// host that does not.
///
/// It refuses rather than looping once it is asked for more than the budget
/// the probe was given, so an unbounded probe **fails this test in
/// milliseconds** instead of hanging the suite or eating the machine's
/// memory. That is deliberate: the defect this guards (`PR5-RD-001`) is
/// non-termination, and a guard against non-termination that itself does
/// not terminate is no guard.
#[derive(Default)]
struct Endless {
    handed: u64,
    ceiling: u64,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.handed + buf.len() as u64 > self.ceiling {
            return Err(io::Error::other(format!(
                "the probe read past its budget: {} bytes handed out, ceiling {}",
                self.handed, self.ceiling
            )));
        }
        buf.fill(b'x');
        self.handed += buf.len() as u64;
        Ok(buf.len())
    }
}

impl Seek for Endless {
    fn seek(&mut self, _to: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

/// The probe **terminates** on a source with no end, and spends exactly the
/// budget it was given — not one byte more (`PR5-RD-001`).
///
/// The byte count is the assertion, not the verdict. `Husk` is what a probe
/// that read the first byte and gave up answers too, so a test that checked
/// only the class would pass for a probe that had stopped being able to see
/// a committed run at all. And the previous test of this shape asserted
/// `Husk` over one finite regular file, which every implementation of this
/// function — including the one that never returned — satisfies.
#[test]
fn the_first_line_probe_spends_its_budget_and_stops() {
    let budget = FIRST_LINE_WINDOW * 4 + 1234;
    let mut endless = Endless {
        handed: 0,
        // Generous, so what fails is the count below rather than the read:
        // an over-reading probe is caught by an assertion that names the
        // number, not by a mysterious io error.
        ceiling: budget + FIRST_LINE_WINDOW,
    };
    assert_eq!(
        first_line_within(&mut endless, budget),
        None,
        "a source with no newline in it has no first line"
    );
    assert_eq!(
        endless.handed, budget,
        "the probe is bounded by the length the file declares, and by nothing else"
    );

    // A device, a fifo or a socket declares no length, so the budget is
    // zero and the probe reads nothing at all. This is the shape a symlink
    // to /dev/zero presents to `first_line`.
    let mut device = Endless {
        handed: 0,
        ceiling: 1,
    };
    assert_eq!(first_line_within(&mut device, 0), None);
    assert_eq!(device.handed, 0, "a source with no length is not read");
}

/// The budget really is *the file's own length*, and a line that runs past
/// the window is still found through it.
///
/// The pair matters: the first half is what makes the probe terminate, the
/// second is what stops that bound from becoming a classification cap — the
/// exact trade `FIRST_LINE_CAP` got wrong and a bound-shaped repair could
/// reintroduce.
#[test]
fn the_budget_is_the_files_length_and_a_line_past_the_window_is_still_read() {
    let root = scratch("budget");
    let window = usize::try_from(FIRST_LINE_WINDOW).expect("the window fits a usize");
    let line = committed_line_of_exactly("01BUDGET", window + 4096);
    let path = root.join("long").join(EVENT_LOG);
    write(&path, &line);

    let mut file = File::open(&path).expect("open");
    assert_eq!(
        file.metadata().expect("stat").len(),
        line.len() as u64,
        "the bound the probe takes is this number"
    );
    assert_eq!(
        first_line(&mut file).expect("a line past the window is still a line"),
        line[..line.len() - 1].to_vec()
    );
    assert_eq!(
        classify_run_dir(root.join("long").as_path()),
        RunDirClass::Committed,
        "a committed run over the window is never excluded by a read bound"
    );
}

/// A file with no newline anywhere is a husk, and is answered without
/// materialising it.
///
/// This is what the window was introduced for and the property the repair
/// had to keep: `newline_offset_from` scans a fixed `SCAN_CHUNK` buffer, so
/// the cost of "there is no newline" is independent of the file's size.
/// Sixteen windows of it, which the pre-repair probe would have read one
/// megabyte of and this one reads all of in 64 KiB at a time.
///
/// It does **not** establish termination and no longer claims to
/// (`PR5-RD-001`): one finite regular file reaches end of file under every
/// implementation of this function, including the one that never returned
/// for a source that has no end. `the_first_line_probe_spends_its_budget_
/// and_stops` and `a_run_directory_whose_log_never_ends_is_still_classified`
/// carry that.
///
/// `Husk` is also the safe direction: a husk is never deleted on shape
/// alone — deletion additionally requires the ownership proof, which
/// requires `committed.json` to be absent, and a run that reached
/// `run_started` published one at P5b.
#[test]
fn a_log_with_no_newline_at_all_is_a_husk_however_long_it_is() {
    let root = scratch("no-newline");
    let window = usize::try_from(FIRST_LINE_WINDOW).expect("the window fits a usize");
    // Valid JSON, so the answer cannot come from the parse.
    let head = committed_line_of_exactly("01NONL", 4096);
    let mut bytes = head[..head.len() - 1].to_vec();
    bytes.extend(std::iter::repeat_n(b'x', window * 16));
    assert!(!bytes.contains(&b'\n'));
    let public = root.join("long");
    write(&public.join(EVENT_LOG), &bytes);
    assert_eq!(classify_run_dir(&public), RunDirClass::Husk);

    let mut file = File::open(public.join(EVENT_LOG)).expect("open");
    assert_eq!(
        first_line(&mut file),
        None,
        "no newline is no first line, not an empty one"
    );
}

/// Where [`endless_log_classification_helper`] is pointed.
const ENDLESS_LOG_DIR: &str = "UPSTROKE_ENDLESS_LOG_DIR";

/// Set when the helper may also *open* the log itself and measure
/// [`first_line`]'s bound over it — true of a device, false of a fifo.
const ENDLESS_LOG_PROBE: &str = "UPSTROKE_ENDLESS_LOG_PROBE";

/// The child half of
/// [`a_run_directory_whose_log_never_ends_is_still_classified`].
///
/// A subprocess rather than a thread, and the reason is the failure mode
/// rather than the success one: a probe that does not terminate cannot be
/// stopped from inside the process it is running in, and the mutation this
/// guards against (an unconditional `read_to_end`) also grows memory
/// without bound while it fails to return. A child can be killed at a
/// deadline; a thread would take the whole suite, and the machine, with it.
#[test]
#[ignore = "subprocess helper"]
fn endless_log_classification_helper() {
    let Ok(dir) = std::env::var(ENDLESS_LOG_DIR) else {
        return;
    };
    assert_eq!(
        classify_run_dir(Path::new(&dir)),
        RunDirClass::Husk,
        "a log with no end holds no newline-terminated run_started"
    );
    // The second axis, and it is only measurable where the source can be
    // opened at all. `classify_run_dir` answering `Husk` is satisfied by a
    // guard that refuses the *name* and by a bound that reads the *bytes*,
    // so on its own it cannot say which one answered — and once
    // `first_committed_line` refuses to open a non-regular file, the
    // endless-device witness would silently stop reaching the bound it was
    // built for (`PR5-RD-001`). Here the child holds the guard's verdict
    // constant and varies the handle: it opens the device itself and asserts
    // the bounded read *also* terminates on it.
    if std::env::var_os(ENDLESS_LOG_PROBE).is_some() {
        let mut device = File::open(Path::new(&dir).join(EVENT_LOG)).expect("the log opens");
        assert_eq!(
            first_line(&mut device),
            None,
            "the bounded read must terminate on the device too, not only the guard"
        );
    }
    std::process::exit(0);
}

/// Run [`endless_log_classification_helper`] against `public` in a child,
/// and fail with `never_returned` if it has not answered within 20 seconds.
///
/// A subprocess, not a thread, for the reason the helper's own comment
/// gives: a probe that does not terminate cannot be stopped from inside its
/// own process, and both shapes this drives — an unbounded `read_to_end`
/// and a blocked `open(2)` — are exactly that.
///
/// Unix-gated because both callers are: `/dev/zero` and `mkfifo` are the two
/// ways to get hold of a non-terminating source without privilege and
/// neither exists on Windows, so on the guest this would be dead code — and
/// the guest's `-D warnings` says so, which is how this gate was found.
#[cfg(unix)]
fn classification_must_answer(public: &Path, probe: bool, never_returned: &str) {
    let helper = format!(
        "{}::endless_log_classification_helper",
        module_path!()
            .split_once("::")
            .expect("this module is not the crate root")
            .1
    );
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("the test executable"));
    command
        .args([helper.as_str(), "--ignored", "--exact"])
        .env(ENDLESS_LOG_DIR, public)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if probe {
        command.env(ENDLESS_LOG_PROBE, "1");
    }
    let mut child = command.spawn().expect("spawn the classification helper");

    let deadline = Instant::now() + Duration::from_secs(20);
    let outcome = loop {
        match child.try_wait().expect("wait on the helper") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let status = outcome.unwrap_or_else(|| panic!("{never_returned}"));
    assert!(
        status.success(),
        "the helper reached a verdict but it was the wrong one, or it died: {status:?}"
    );
}

/// A public run directory whose `events.jsonl` is a **fifo with no writer**
/// is classified, and classified promptly (`PR5-CONF-001`).
///
/// The sibling below plants an endless *device*, whose `open` returns and
/// whose `read` never ends; this plants a source whose `open` itself never
/// returns, which no bound on the read can defend against because the bound
/// is taken by `fstat` on a handle that is never produced. `startup_census`
/// requires every entry to classify before a write command proceeds and the
/// command holds the physical worktree lock across the census, so the
/// consequence is the same one `PR5-RD-001` was repaired for: a lock held
/// for ever by a process that will never make progress.
///
/// The two axes this crosses are the *file type* and the *syscall that
/// meets it*. Held constant: the directory shape, which is a perfectly
/// ordinary public run directory — the only thing that varies from a
/// `Committed` one is the type of the `events.jsonl` entry.
///
/// Unix only, because a fifo is where a blocking `open` can be got hold of
/// without privilege; `mkfifo` has no Windows counterpart at all.
#[cfg(unix)]
#[test]
fn a_run_directory_whose_log_blocks_on_open_is_still_classified() {
    use std::os::unix::fs::FileTypeExt as _;

    let root = scratch("fifo");
    let public = root.join("run");
    fs::create_dir_all(&public).expect("public");
    let log = public.join(EVENT_LOG);
    let name = std::ffi::CString::new(log.as_os_str().as_encoded_bytes())
        .expect("a scratch path holds no interior NUL");
    // SAFETY: `name` is a live NUL-terminated path in a directory this test
    // just created; `mkfifo` borrows it for the duration of the call.
    let made = unsafe { libc::mkfifo(name.as_ptr(), 0o600) };
    assert_eq!(
        made,
        0,
        "could not plant the fifo: {}",
        std::io::Error::last_os_error()
    );
    assert!(
        fs::symlink_metadata(&log)
            .expect("stat the fifo")
            .file_type()
            .is_fifo(),
        "the planted entry must really be a fifo, or nothing here is measured"
    );
    // The premise, stated rather than assumed: `stat` answers about this
    // entry immediately, so a guard that consults the type before opening
    // can terminate — and that is the only reason one is possible.
    assert_eq!(
        fs::symlink_metadata(&log).expect("stat the fifo").len(),
        0,
        "a fifo declares no length"
    );

    classification_must_answer(
        &public,
        false,
        "classify_run_dir did not return within 20s for an events.jsonl that is a \
             writer-less fifo. `File::open` blocks in the kernel before any bound on the \
             read applies, the startup census would never classify this entry, and the \
             write command would hold the worktree lock for ever (PR5-CONF-001)",
    );
}

/// A public run directory whose `events.jsonl` is a real endless device is
/// classified, and classified *quickly* (`PR5-RD-001`).
///
/// `startup_census` requires **every** run-directory entry to be classified
/// `Committed` or `Husk` before a write command proceeds, and the write
/// command holds the physical worktree lock while it does that. An entry
/// that never classifies is therefore not a slow census: it is a lock held
/// for ever by a process that will never make progress, and no later
/// command in that worktree can run.
///
/// Unix only because `/dev/zero` is where a source with no end can be got
/// hold of without privilege. The platform-free half of the same claim —
/// that the probe spends a finite budget and stops — is
/// `the_first_line_probe_spends_its_budget_and_stops`, which runs on the
/// Windows guest too.
///
/// **The child probes the device as well as the directory**, and that is not
/// decoration (`PR5-CONF-001`). Once `first_committed_line` refuses to open
/// anything that is not a regular file, this planted symlink is answered by
/// the *guard*, so the classification alone would no longer reach the bound
/// this test exists for — a green `Husk` would mean the name was refused and
/// say nothing about the read. The child therefore holds the class constant
/// and varies the handle: it asserts `Husk`, then opens the real device and
/// asserts the bounded read terminates on it too. Both assertions run inside
/// the same 20-second deadline, so either one failing to *return* fails
/// here rather than hanging the suite.
#[cfg(unix)]
#[test]
fn a_run_directory_whose_log_never_ends_is_still_classified() {
    let root = scratch("endless");
    let public = root.join("run");
    fs::create_dir_all(&public).expect("public");
    assert!(
        Path::new("/dev/zero").exists(),
        "this host has no endless device, so nothing here is measured"
    );
    std::os::unix::fs::symlink("/dev/zero", public.join(EVENT_LOG)).expect("symlink");
    // The device is what the probe will actually meet: a handle that opens,
    // declares no length, and never reaches end of file.
    let device = File::open(public.join(EVENT_LOG)).expect("the log opens");
    assert_eq!(
        device.metadata().expect("stat").len(),
        0,
        "a character device declares no length, which is the probe's budget"
    );
    drop(device);

    classification_must_answer(
        &public,
        true,
        "classify_run_dir or first_line did not return within 20s for an events.jsonl \
             that never ends. The startup census would never classify this entry and the \
             write command would hold the worktree lock for ever (PR5-RD-001)",
    );
}

// =======================================================================
// Readers by commitment
// =======================================================================

/// A repository holding one committed run, one husk older than it and one
/// husk newer than it — so a reader that returned husks would be caught
/// whichever end of the sort it went wrong at.
fn repo_with_a_committed_run_between_two_husks(tag: &str) -> PathBuf {
    let repo = scratch(tag).join("repo");
    fs::create_dir_all(runs_root(&repo).join("01AAAHUSK")).expect("older husk");
    write(
        &public_dir(&repo, "01AAAHUSK").join(PLAN),
        b"{\"tasks\":[]}",
    );
    commit_run(&repo, "01BBBRUN");
    fs::create_dir_all(runs_root(&repo).join("01ZZZHUSK")).expect("newer husk");
    write(
        &public_dir(&repo, "01ZZZHUSK").join(MARKER),
        &any_marker_bytes(),
    );
    repo
}

/// Every reader **in this module**, crossed with every husk **shape** —
/// the second axis, and the one this fixture used to be too narrow on.
///
/// `startup_census` names five readers — `list_runs`, `latest_run`,
/// `resolve_run_id`, `find_question`, `status` — and four of them live
/// here. The fifth is the `status` command, which reaches run directories
/// through `resolve_run_id` and `husk_report`, and its husk behaviour is
/// pinned in its own module by
/// `status_asked_for_a_husk_id_names_which_husk_it_is`.
///
/// Four readers against two shapes caught any reader that simply stopped
/// filtering: `find_question` scanning `run_dir_names`, and `latest_run`
/// taking the newest directory, both die here. What it could not see was a
/// shape it did not build. Its husks are a markerless directory carrying
/// content and one with a **well-formed** marker, and
/// `a_committed_run_is_never_excluded_because_of_a_marker` uses well-formed
/// markers too — so a filter that admitted exactly the *malformed-marker*
/// husk changed no measured answer, and the readers' behaviour over that
/// shape was unpinned in both directions. Measured surviving the whole
/// suite on Linux and on the Windows guest.
///
/// `01ZZZMALFORMED` is therefore built to win every reader it could: it
/// sorts lexically last, so `latest_run` would take it, and it carries the
/// question id being searched for, so `find_question` would return it.
#[test]
fn every_reader_returns_committed_directories_only() {
    let repo = repo_with_a_committed_run_between_two_husks("readers");
    // The third shape: a marker that is present and unparseable. Not a
    // fifth reader — the four this module owns are all here already, and
    // the fifth, `status`, is pinned in `status.rs` — a third *shape*.
    let malformed = public_dir(&repo, "01ZZZMALFORMED");
    fs::create_dir_all(&malformed).expect("malformed-marker husk");
    write(&malformed.join(MARKER), b"{ not json at all");
    for husk in ["01AAAHUSK", "01ZZZHUSK", "01ZZZMALFORMED"] {
        let questions = public_dir(&repo, husk).join("questions");
        fs::create_dir_all(&questions).expect("questions");
        fs::write(questions.join("q-HUSK.json"), "{}").expect("question");
    }
    let questions = public_dir(&repo, "01BBBRUN").join("questions");
    fs::create_dir_all(&questions).expect("questions");
    fs::write(questions.join("q-REAL.json"), "{}").expect("question");

    assert_eq!(list_runs(&repo), ["01BBBRUN"], "list_runs");
    assert_eq!(latest_run(&repo).as_deref(), Some("01BBBRUN"), "latest_run");
    assert_eq!(
        resolve_run_id(&repo, "01BBBRUN").expect("the committed run resolves"),
        "01BBBRUN"
    );
    for husk in ["01AAAHUSK", "01ZZZHUSK", "01ZZZMALFORMED"] {
        let error = resolve_run_id(&repo, husk).expect_err("a husk is not a run");
        assert!(
            error.to_string().contains("never recorded a committed"),
            "resolve_run_id must say why: {error}"
        );
    }
    assert_eq!(
        find_question(&repo, "q-REAL")
            .expect("the committed run's question")
            .run_id,
        "01BBBRUN"
    );
    let error = find_question(&repo, "q-HUSK").expect_err("a husk's question is not findable");
    assert!(error.to_string().contains("no question"), "{error}");

    // And the husks are still there: a reader observes, it never reclaims.
    assert_eq!(
        list_husks(&repo),
        ["01AAAHUSK", "01ZZZHUSK", "01ZZZMALFORMED"]
    );
    assert_eq!(run_dir_names(&repo).len(), 4);
}

#[test]
fn a_committed_run_is_never_excluded_because_of_a_marker() {
    // The other half of the behaviour change, and the half a plausible
    // suite forgets: `run_creation` says readers "never return a directory
    // without a committed run_started **and never hide one because of a
    // marker**". Both marker shapes, and with a newer husk present so the
    // committed run has to win `latest_run` on its merits.
    let repo = scratch("markedcommitted").join("repo");
    commit_run(&repo, "01AAAMARKED");
    commit_run(&repo, "01BBBSTAGED");
    write(
        &public_dir(&repo, "01AAAMARKED").join(MARKER),
        &any_marker_bytes(),
    );
    write(
        &public_dir(&repo, "01BBBSTAGED").join(MARKER_STAGED),
        &any_marker_bytes(),
    );
    fs::create_dir_all(runs_root(&repo).join("01ZZZHUSK")).expect("newer husk");

    assert_eq!(list_runs(&repo), ["01AAAMARKED", "01BBBSTAGED"]);
    assert_eq!(
        latest_run(&repo).as_deref(),
        Some("01BBBSTAGED"),
        "a committed-but-marked run is the latest run, and a husk newer \
             than it does not become one"
    );
    for id in ["01AAAMARKED", "01BBBSTAGED"] {
        assert_eq!(resolve_run_id(&repo, id).expect("resolves"), id);
        assert_eq!(
            classify_run_dir(&public_dir(&repo, id)),
            RunDirClass::Committed
        );
    }
}

#[test]
fn latest_run_skips_a_husk_that_would_otherwise_shadow_it() {
    // The named change: "legacy husks that today shadow latest_run are no
    // longer listed". Asserted from the shadowing direction, because that
    // is the operator-visible symptom.
    let repo = repo_with_a_committed_run_between_two_husks("shadow");
    assert_eq!(latest_run(&repo).as_deref(), Some("01BBBRUN"));
    assert!(
        run_dir_names(&repo)
            .last()
            .is_some_and(|last| last == "01ZZZHUSK"),
        "the husk really is the newest directory, so the skip is doing work"
    );
}

// =======================================================================
// The private half's ownership
// =======================================================================

const BOUND_RUN: &str = "01BOUNDHUSK000000000000000";
const BOUND_INCARNATION: &str = "01INCARNATION00000000000000";

/// A husk at P3b–P5: the marker published, the private half created, the
/// owner record published, and no commit record. The one shape the proof
/// is supposed to accept.
struct BoundHusk {
    root: PathBuf,
    repo: PathBuf,
    private_root: PathBuf,
    repo_key: RepoKey,
    /// Where the private half's bytes are written.
    private: PathBuf,
    marker: CreatingMarker,
    owner: OwnerRecord,
}

impl BoundHusk {
    fn new(tag: &str) -> Self {
        Self::at(scratch(tag))
    }

    /// The same husk, under a root the caller already owns.
    ///
    /// Extracted from [`Self::new`] so that a fixture can be built inside
    /// an acquired `scratch_tree` root rather than beside one: the
    /// committed-record witness needs both — a husk the ownership proof
    /// refuses, and a root the scratch token authorises reclaiming.
    fn at(root: PathBuf) -> Self {
        let repo = root.join("repo");
        let private_root = root.join("private");
        let public = public_dir(&repo, BOUND_RUN);
        fs::create_dir_all(&public).expect("public");
        fs::create_dir_all(private_root.join("runs")).expect("runs root");
        let private = fs::canonicalize(private_root.join("runs"))
            .expect("canonical runs root")
            .join(BOUND_RUN);
        let repo_key = RepoKey::v1(&root.join("git-dir"));
        let policy = crate::runner::policy::host_policy();
        let marker = CreatingMarker {
            run_id: BOUND_RUN.to_owned(),
            repo_key: repo_key.as_str().to_owned(),
            private_dir: private.to_string_lossy().into_owned(),
            incarnation: BOUND_INCARNATION.to_owned(),
            pid: std::process::id(),
            runner_policy_sha256: runner_policy_sha256(&policy),
        };
        let owner = OwnerRecord {
            run_id: BOUND_RUN.to_owned(),
            repo_key: repo_key.as_str().to_owned(),
            public_dir: fs::canonicalize(&public)
                .expect("canonical public")
                .to_string_lossy()
                .into_owned(),
            incarnation: BOUND_INCARNATION.to_owned(),
            runner: policy,
        };
        Self {
            root,
            repo,
            private_root,
            repo_key,
            private,
            marker,
            owner,
        }
    }

    fn public(&self) -> PathBuf {
        public_dir(&self.repo, BOUND_RUN)
    }

    /// Publish both halves through the funnels, in the packet's order.
    fn publish(&self) {
        let hooks = &mut NoHooks;
        let public = self.public();
        create_public_dir(&public, hooks).expect("P0");
        stage_marker(&public, &self.marker, hooks).expect("P1a");
        publish_marker(&public, hooks).expect("P1b");
        create_private_dir(&self.private, hooks).expect("P3");
        stage_owner_record(&self.private, &self.owner, hooks).expect("P3a");
        publish_owner_record(&self.private, hooks).expect("P3b");
    }

    fn prove(&self) -> PrivateHalfOwnership {
        prove_private_half_ownership(&self.public(), &self.repo_key, &self.private_root)
    }
}

/// Every file below `root`, by relative path, so "byte-identical
/// afterwards" is an assertion rather than a hope.
fn snapshot_tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = fs::read(&path) {
                out.insert(
                    path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                    bytes,
                );
            }
        }
    }
    out
}

/// A directory link: a POSIX symlink, or on Windows a **junction**.
///
/// `mklink /J` rather than `/D` because a junction needs no privilege and
/// is exactly the reparse point `expected_failures_refusals[0]` names
/// beside a symlink. A refusal that only fired on POSIX symlinks would
/// pass every Linux test and refuse nothing on the platform the word
/// "junction" is about.
fn link_dir(link: &Path, target: &Path) {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).expect("symlink");
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .expect("mklink runs");
        assert!(
            status.success(),
            "creating a junction must succeed; an unmakeable junction is a \
                 failure of this test, never a skip"
        );
    }
    assert!(
        fs::symlink_metadata(link).is_ok(),
        "the link must exist afterwards"
    );
}

/// What a case expects the proof to answer.
#[derive(Debug, PartialEq, Eq)]
enum Expect {
    Proven,
    Nothing(UnboundShape),
    /// The kind, and — when the owner record is what disagreed — the field.
    Retained(&'static str, Option<OwnerField>),
}

struct ProofCase {
    name: &'static str,
    /// Applied to the records before publication.
    before: fn(&mut BoundHusk),
    /// Applied to the bytes on disk after publication.
    after: fn(&BoundHusk),
    expect: Expect,
}

fn nothing(_: &mut BoundHusk) {}
fn nothing_after(_: &BoundHusk) {}

/// One case per conjunct, because every conjunct is separately droppable
/// and a suite testing the happy path plus one negative passes with any
/// single one removed.
fn proof_cases() -> Vec<ProofCase> {
    vec![
        ProofCase {
            name: "a bound husk without a commit record yields a token",
            before: nothing,
            after: nothing_after,
            expect: Expect::Proven,
        },
        ProofCase {
            name: "malformed marker",
            before: nothing,
            after: |husk| write(&husk.public().join(MARKER), b"{ not json at all"),
            expect: Expect::Retained("marker-unparseable", None),
        },
        ProofCase {
            name: "forged marker naming a foreign run",
            before: |husk| husk.marker.run_id = "01FOREIGNRUN00000000000000".to_owned(),
            after: nothing_after,
            expect: Expect::Retained("marker-run-id-mismatch", None),
        },
        ProofCase {
            name: "copied husk from another repository",
            before: |husk| {
                husk.marker.repo_key = RepoKey::v1(&husk.root.join("another-git-dir"))
                    .as_str()
                    .to_owned();
            },
            after: nothing_after,
            expect: Expect::Retained("marker-repo-key-mismatch", None),
        },
        ProofCase {
            // The existence step, refusing on a stat that is not an answer.
            // The input is a path holding a NUL, which `std` rejects before
            // any syscall: measured on Linux as `InvalidInput` ("file name
            // contained an unexpected NUL byte"), and expected to be
            // `InvalidInput` on Windows too, where the wide-string conversion
            // refuses interior NULs. **That second half is expected, not
            // measured here** — CI's Windows leg is what settles it, and this
            // case is what asks the question. Either way it is not `NotFound`,
            // which is the only kind this conjunct now takes as proof, and it
            // needs no privilege and no mode bits.
            //
            // A reachable input rather than a contrivance: `private_dir` is a
            // JSON string read off disk, and `\u0000` is valid JSON. The
            // `EACCES` shape an operator actually meets is the Unix witness
            // below.
            name: "a marker whose recorded target cannot be asked about",
            before: |husk| {
                husk.marker.private_dir = format!("{}\0nul", husk.private.display());
            },
            after: nothing_after,
            expect: Expect::Retained("target-undecidable", None),
        },
        ProofCase {
            name: "locator outside the authorized private root",
            before: |husk| {
                let foreign = husk.root.join("foreign-root").join("runs");
                fs::create_dir_all(&foreign).expect("foreign root");
                husk.private = fs::canonicalize(&foreign)
                    .expect("canonical foreign root")
                    .join(BOUND_RUN);
                husk.marker.private_dir = husk.private.to_string_lossy().into_owned();
            },
            after: nothing_after,
            expect: Expect::Retained("locator-outside-authorized-root", None),
        },
        ProofCase {
            name: "locator through a reparse point",
            before: |husk| {
                let real = husk.private_root.join("elsewhere");
                fs::create_dir_all(&real).expect("real private half");
                let link = husk.private_root.join("runs").join(BOUND_RUN);
                link_dir(&link, &real);
                // The marker records the *link*, which is what a census
                // has to follow and what the chain check has to refuse.
                husk.private = link.clone();
                husk.marker.private_dir = link.to_string_lossy().into_owned();
            },
            after: nothing_after,
            expect: Expect::Retained("locator-through-reparse-point", None),
        },
        ProofCase {
            name: "private target without an owner record",
            before: nothing,
            after: |husk| {
                fs::remove_file(husk.private.join(OWNER_RECORD)).expect("remove owner record");
            },
            expect: Expect::Retained("owner-record-missing", None),
        },
        ProofCase {
            name: "owner record that cannot be read",
            before: nothing,
            after: |husk| write(&husk.private.join(OWNER_RECORD), b"{ not json"),
            expect: Expect::Retained("owner-record-unparseable", None),
        },
        ProofCase {
            name: "owner record disagreeing on run id",
            before: |husk| husk.owner.run_id = "01OTHERRUN0000000000000000".to_owned(),
            after: nothing_after,
            expect: Expect::Retained("owner-record-disagrees", Some(OwnerField::RunId)),
        },
        ProofCase {
            name: "owner record disagreeing on repo key",
            before: |husk| {
                husk.owner.repo_key = RepoKey::v1(&husk.root.join("third-git-dir"))
                    .as_str()
                    .to_owned();
            },
            after: nothing_after,
            expect: Expect::Retained("owner-record-disagrees", Some(OwnerField::RepoKey)),
        },
        ProofCase {
            name: "owner record disagreeing on public path",
            before: |husk| {
                husk.owner.public_dir = husk
                    .root
                    .join("some-other-run-directory")
                    .to_string_lossy()
                    .into_owned();
            },
            after: nothing_after,
            expect: Expect::Retained("owner-record-disagrees", Some(OwnerField::PublicDir)),
        },
        ProofCase {
            name: "owner record disagreeing on incarnation",
            before: |husk| husk.owner.incarnation = "01ANOTHERINCARNATION000000".to_owned(),
            after: nothing_after,
            expect: Expect::Retained("owner-record-disagrees", Some(OwnerField::Incarnation)),
        },
        ProofCase {
            name: "owner record naming another runner boundary",
            before: |husk| husk.owner.runner = another_policy(),
            after: nothing_after,
            expect: Expect::Retained("owner-record-disagrees", Some(OwnerField::RunnerDigest)),
        },
        ProofCase {
            name: "marker-less husk carrying run-scoped content",
            before: nothing,
            after: |husk| {
                fs::remove_file(husk.public().join(MARKER)).expect("remove marker");
                write(&lock_file(&husk.public()), b"");
            },
            expect: Expect::Retained("markerless-with-content", None),
        },
        ProofCase {
            // The other answer `unbound_shape` gives, and the one this grid
            // could not reach before `SWEEP-CLASSIFY-009`: a listing that did
            // not happen used to be an empty one, which is the *reclaiming*
            // answer. Removing the public directory is how a `read_dir` is made
            // to fail on every platform this crate builds for; the `EACCES`
            // shape the finding is actually about — a transient the whole
            // process hits, where the marker read and the listing fail together
            // and clear before the removal runs — needs mode bits and is the
            // Unix witness below.
            name: "a public half whose listing does not answer",
            before: nothing,
            after: |husk| {
                fs::remove_dir_all(husk.public()).expect("remove the public half");
            },
            expect: Expect::Retained("listing-unreadable", None),
        },
        ProofCase {
            name: "private half carrying a commit record",
            before: nothing,
            after: |husk| write(&husk.private.join(COMMIT_RECORD), b"{}"),
            expect: Expect::Retained("possibly-committed", None),
        },
        ProofCase {
            name: "bare public directory",
            before: nothing,
            after: |husk| {
                fs::remove_file(husk.public().join(MARKER)).expect("remove marker");
            },
            expect: Expect::Nothing(UnboundShape::Bare),
        },
        ProofCase {
            name: "staged marker only",
            before: nothing,
            after: |husk| {
                fs::rename(
                    husk.public().join(MARKER),
                    husk.public().join(MARKER_STAGED),
                )
                .expect("unpublish the marker");
            },
            expect: Expect::Nothing(UnboundShape::StagedMarkerOnly),
        },
        ProofCase {
            name: "marker whose recorded target is gone",
            before: nothing,
            after: |husk| {
                fs::remove_dir_all(&husk.private).expect("remove the private half");
            },
            expect: Expect::Nothing(UnboundShape::TargetAbsent),
        },
    ]
}

/// A second host policy, distinguishable from `host_policy()` by its
/// canonical bytes and therefore by its digest.
fn another_policy() -> RunnerPolicy {
    let mut policy = crate::runner::policy::host_policy();
    policy.credential_volumes = Some(std::collections::BTreeMap::from([(
        "claude-code".to_owned(),
        "upstroke-creds".to_owned(),
    )]));
    policy
}

/// Conjunct 5 binds the locator to **this run's basename**, not merely to
/// the authorized `runs` directory (`PR5-RUNDIR-022`).
///
/// `scope` is "locator chain without reparse points canonicalizing to
/// `<authorized private root>/runs/<basename>`" — an equality. The grid's
/// own conjunct-5 case points the locator at a *foreign root*, which a
/// `starts_with` prefix test rejects exactly as an equality does, so the
/// conjunct was proven to reject another root and never asked the question
/// the sentence is about. These are the two shapes a prefix test admits: a
/// **sibling** run's private half, and a path **nested** inside this run's
/// own. The first is the one that matters — under it a proof for run A
/// authorizes deleting run B's private half, and
/// `tests_acceptance.seam_tests[3]` says "no census can bind another run's
/// private half to a husk".
///
/// A separate test rather than two more `proof_cases` entries: that grid
/// asserts one *distinct* `RetainReason` per case, so two more cases
/// refusing for the same reason would fail it, and the property it is
/// asserting — every conjunct separately covered — is worth keeping.
#[test]
fn a_locator_beside_or_below_this_runs_private_half_cannot_authorize_deletion() {
    type Build = fn(&mut BoundHusk) -> PathBuf;
    let cases: Vec<(&str, Build)> = vec![
        (
            "a sibling run under the authorized runs directory",
            |husk| {
                let sibling = husk
                    .private_root
                    .join("runs")
                    .join("01SIBLINGRUN0000000000000");
                fs::create_dir_all(&sibling).expect("the sibling private half");
                write(&sibling.join("evidence"), b"another run's private half");
                fs::canonicalize(&sibling).expect("canonical sibling")
            },
        ),
        ("a path nested below this run's private half", |husk| {
            let nested = husk
                .private_root
                .join("runs")
                .join(BOUND_RUN)
                .join("transcripts");
            fs::create_dir_all(&nested).expect("the nested directory");
            fs::canonicalize(&nested).expect("canonical nested")
        }),
    ];
    for (index, (name, build)) in cases.into_iter().enumerate() {
        let mut husk = BoundHusk::new(&format!("locator-prefix{index}"));
        let target = build(&mut husk);
        husk.private = target.clone();
        husk.marker.private_dir = target.to_string_lossy().into_owned();
        husk.publish();
        let before = snapshot_tree(&target);

        match husk.prove() {
            PrivateHalfOwnership::Retained(reason) => assert_eq!(
                reason.kind(),
                "locator-outside-authorized-root",
                "{name}: {reason}"
            ),
            other => panic!(
                "{name}: a locator that is not <authorized>/runs/<basename> handed out \
                     {other:?}"
            ),
        }
        assert_eq!(
            snapshot_tree(&target),
            before,
            "{name}: and the refusal touched nothing"
        );
    }
}

#[test]
fn every_conjunct_of_the_ownership_proof_refuses_on_its_own() {
    let mut kinds: Vec<(&'static str, Option<OwnerField>)> = Vec::new();
    let mut shapes: Vec<UnboundShape> = Vec::new();
    let mut proven = 0usize;

    for (index, case) in proof_cases().into_iter().enumerate() {
        let mut husk = BoundHusk::new(&format!("proof{index}"));
        (case.before)(&mut husk);
        husk.publish();
        (case.after)(&husk);
        let before_bytes = snapshot_tree(&husk.private);

        let answer = husk.prove();
        match (&case.expect, &answer) {
            (Expect::Proven, PrivateHalfOwnership::Proven(token)) => {
                assert_eq!(token.run_id(), BOUND_RUN, "{}", case.name);
                assert_eq!(
                    fs::canonicalize(token.target()).expect("canonical target"),
                    fs::canonicalize(&husk.private).expect("canonical private"),
                    "{}",
                    case.name
                );
                proven += 1;
            }
            (Expect::Nothing(expected), PrivateHalfOwnership::NothingBound(shape)) => {
                assert_eq!(shape, expected, "{}", case.name);
                shapes.push(*shape);
            }
            (Expect::Retained(kind, field), PrivateHalfOwnership::Retained(reason)) => {
                assert_eq!(&reason.kind(), kind, "{}: {reason}", case.name);
                assert_eq!(&reason.owner_field(), field, "{}: {reason}", case.name);
                kinds.push((reason.kind(), reason.owner_field()));
            }
            (expected, actual) => {
                panic!("{}: expected {expected:?}, got {actual:?}", case.name)
            }
        }

        // "each yield a RetainReason and leave the target byte-identical".
        assert_eq!(
            snapshot_tree(&husk.private),
            before_bytes,
            "{}: the proof is read-only",
            case.name
        );
    }

    assert_eq!(proven, 1, "exactly one case is the happy path");

    // The counts are what makes a dropped conjunct fail. A suite that
    // asserted only "some negative refuses" passes with any single
    // conjunct deleted; a suite that asserts every *kind* appears exactly
    // once does not.
    let mut distinct = kinds.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        kinds.len(),
        "two cases produced the same reason, so one conjunct is untested: {kinds:?}"
    );

    let mut covered: Vec<&str> = kinds.iter().map(|(kind, _)| *kind).collect();
    covered.sort_unstable();
    covered.dedup();
    let mut expected: Vec<&str> = RetainReason::KINDS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        covered, expected,
        "every RetainReason variant is a conjunct this grid must exercise"
    );

    let mut fields: Vec<OwnerField> = kinds.iter().filter_map(|(_, field)| *field).collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        OwnerField::ALL.to_vec(),
        "every field the owner record is checked on has its own case"
    );

    shapes.sort_unstable_by_key(|shape| format!("{shape:?}"));
    let mut expected_shapes = UnboundShape::ALL.to_vec();
    expected_shapes.sort_unstable_by_key(|shape| format!("{shape:?}"));
    assert_eq!(shapes, expected_shapes, "every unbound shape has a case");
}

#[test]
fn a_marker_digest_naming_another_boundary_is_the_same_refusal() {
    // The mismatch the packet calls `runner_digest_mismatch_retained` can
    // be written from either side; both are one comparison and both must
    // refuse. The grid mutates the record's policy, so this mutates the
    // marker's digest.
    let mut husk = BoundHusk::new("markerdigest");
    husk.marker.runner_policy_sha256 = runner_policy_sha256(&another_policy());
    husk.publish();
    match husk.prove() {
        PrivateHalfOwnership::Retained(reason) => {
            assert_eq!(reason.kind(), "owner-record-disagrees");
            assert_eq!(reason.owner_field(), Some(OwnerField::RunnerDigest));
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// `owner.json` **absent** and `committed.json` **present**: the fourth
/// cell of two axes the grid covers only one at a time.
///
/// The grid has both singles — "private target without an owner record"
/// answers `owner-record-missing`, "private half carrying a commit record"
/// answers `possibly-committed` — and neither is the crossing. Measured:
/// an arm answering `Proven` for exactly this cell survived the whole
/// suite, and `Proven` here is a `PrivateHalfProof`, the deletion token and
/// the only key to `remove_private_husk`, handed out for a half that may
/// have crossed P5b.
///
/// **Standalone rather than a `ProofCase` row, and it has to be.** The grid
/// asserts that no two cases produce the same `RetainReason` ("two cases
/// produced the same reason, so one conjunct is untested"), and this cell
/// answers `owner-record-missing`, which the single-axis case already
/// claims — so the crossing cannot be written as a row at all. The grid's
/// own shape is part of why the crossing was missing.
///
/// The conjunct **order** is what decides which reason comes out: the owner
/// check precedes the commit check, so this cell reports the missing record
/// rather than the possible commit. Both are safe — neither yields a token
/// — but they are different things to tell an operator, and nothing else
/// pins which one it is.
#[test]
fn probe_a_commit_record_without_an_owner_record_yields_no_token() {
    let husk = BoundHusk::new("probe-commit-no-owner");
    husk.publish();
    fs::remove_file(husk.private.join(OWNER_RECORD)).expect("remove the owner record");
    write(&husk.private.join(COMMIT_RECORD), b"{}");
    let before = snapshot_tree(&husk.private);

    match husk.prove() {
        PrivateHalfOwnership::Retained(reason) => {
            assert_eq!(reason.kind(), "owner-record-missing");
        }
        other => {
            panic!("a private half that may have crossed P5b must never be proven: {other:?}")
        }
    }
    assert_eq!(
        snapshot_tree(&husk.private),
        before,
        "the private half is byte-identical after the proof"
    );
}

/// `owner.json.tmp` present and `owner.json` absent — what an interrupted
/// P3b leaves — is not a record, and yields no token.
///
/// Neither axis alone can see the difference. `PR5-RUNDIR-045`'s fixture
/// leaves the staging file where the published record is also present, and
/// `PR5-RUNDIR-024`'s has neither file, so a proof that fell back from
/// `owner.json` to `owner.json.tmp` changed no measured answer: an
/// interrupted publication read as a completed one, and a record that was
/// never durable became proof of ownership. That fallback survived the
/// whole suite.
///
/// The fixture is built by **unpublishing** — renaming the published record
/// back to its staging name — so the half on disk is exactly the state P3a
/// leaves and P3b has not yet finished. Both halves are compared byte for
/// byte afterwards, the staging file included, so an implementation that
/// consumed or tidied it up cannot pass either.
#[test]
fn probe_an_owner_staging_file_is_not_an_owner_record() {
    let husk = BoundHusk::new("probe-owner-staged-only");
    husk.publish();
    fs::rename(
        husk.private.join(OWNER_RECORD),
        husk.private.join(OWNER_RECORD_STAGED),
    )
    .expect("unpublish the owner record");
    let before_private = snapshot_tree(&husk.private);
    let before_public = snapshot_tree(&husk.public());

    match husk.prove() {
        PrivateHalfOwnership::Retained(reason) => {
            assert_eq!(reason.kind(), "owner-record-missing");
        }
        other => panic!("an interrupted publication is not a proof of ownership: {other:?}"),
    }
    assert!(
        husk.private.join(OWNER_RECORD_STAGED).is_file(),
        "the staging file is still where the interruption left it"
    );
    assert_eq!(
        snapshot_tree(&husk.private),
        before_private,
        "the private half is byte-identical after the proof"
    );
    assert_eq!(
        snapshot_tree(&husk.public()),
        before_public,
        "and so is the public half"
    );
}

#[test]
fn the_names_on_disk_are_the_names_the_packet_writes() {
    // The funnels and the proof share the path constants, so a rename of
    // one constant would move both together and every other test in this
    // module would still pass. These are literals, written out of
    // `run_creation` and `resource_accounting`.
    let husk = BoundHusk::new("names");
    let public = husk.public();
    stage_marker(&public, &husk.marker, &mut NoHooks).expect("stage");
    assert!(public.join(".creating.tmp").is_file(), "staged marker");
    publish_marker(&public, &mut NoHooks).expect("publish");
    assert!(public.join(".creating").is_file(), "published marker");
    assert!(!public.join(".creating.tmp").exists(), "staging is spent");

    create_private_dir(&husk.private, &mut NoHooks).expect("private");
    stage_owner_record(&husk.private, &husk.owner, &mut NoHooks).expect("stage owner");
    assert!(husk.private.join("owner.json.tmp").is_file());
    publish_owner_record(&husk.private, &mut NoHooks).expect("publish owner");
    assert!(husk.private.join("owner.json").is_file());
    assert!(!husk.private.join("owner.json.tmp").exists());

    let record = CommitRecord {
        run_id: BOUND_RUN.to_owned(),
        repo_key: husk.repo_key.as_str().to_owned(),
        public_dir: husk.owner.public_dir.clone(),
        incarnation: BOUND_INCARNATION.to_owned(),
        run_started_sha256: run_started_sha256(committed_line(BOUND_RUN, 4).as_bytes()),
    };
    stage_commit_record(&husk.private, &record, &mut NoHooks).expect("stage commit");
    assert!(husk.private.join("committed.json.tmp").is_file());
    publish_commit_record(&husk.private, &mut NoHooks).expect("publish commit");
    assert!(husk.private.join("committed.json").is_file());
    assert!(!husk.private.join("committed.json.tmp").exists());

    assert_eq!(
        public.join(EVENT_LOG).file_name().expect("name"),
        "events.jsonl"
    );
    assert_eq!(
        public.join(PLAN).file_name().expect("name"),
        "plan.normalized.json"
    );
    assert_eq!(lock_file(&public).file_name().expect("name"), "run.lock");
    assert_eq!(
        worktree_lock_file(Path::new("g"))
            .file_name()
            .expect("name"),
        "upstroke-worktree.lock"
    );
}

#[test]
fn a_committed_private_half_is_never_provable_however_bound_it_is() {
    // The commit-record condition is the last conjunct and the one whose
    // absence is invisible in the happy path: every other field agrees, so
    // a proof that had dropped it would hand out a token for a private
    // half that may have crossed P5b.
    let husk = BoundHusk::new("committedhalf");
    husk.publish();
    assert!(
        matches!(husk.prove(), PrivateHalfOwnership::Proven(_)),
        "the same husk without a commit record is provable"
    );
    write(&husk.private.join(COMMIT_RECORD), b"{}");
    match husk.prove() {
        PrivateHalfOwnership::Retained(RetainReason::PossiblyCommitted) => {}
        other => panic!("a commit record must refuse the token: {other:?}"),
    }
}

/// Conjunct 12 is fail-closed: only `NotFound` proves the record absent.
///
/// The conjunct was `fs::symlink_metadata(..).is_ok()`, so every stat error
/// that is *not* `NotFound` — `EACCES` on a directory that became
/// unreadable between the owner-record read and this stat, `EIO`, a Windows
/// sharing violation — read as "absent" and fell through to `Proven`,
/// minting the one token `remove_private_husk` accepts for a private half
/// whose `committed.json` could not be ruled out.
/// `commit_record_after_error` answers the same question the other way
/// (`Unknown`, which `permits_deletion()` refuses), so the two paths into
/// the one deletion boundary disagreed and this was the open one.
///
/// The classification is asserted directly because it is not
/// deterministically reachable through the filesystem from one thread: a
/// private directory made unreadable refuses at conjunct 6's owner-record
/// read, long before this stat. The two shapes that *are* reachable —
/// present and absent — are asserted through the whole proof by
/// `a_committed_private_half_is_never_provable_however_bound_it_is` and by
/// the wiring half below.
#[test]
fn a_commit_record_stat_that_is_not_not_found_is_not_proof_of_absence() {
    use std::io::{Error, ErrorKind};

    let husk = BoundHusk::new("conjunct12");
    husk.publish();

    // (1) The classification, over every shape the stat can produce.
    assert!(
        ownership::commit_record_proves_absence(&Err(Error::from(ErrorKind::NotFound))),
        "`NotFound` is the one answer that proves the record is not there"
    );
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::Other,
        ErrorKind::InvalidInput,
        ErrorKind::TimedOut,
    ] {
        assert!(
            !ownership::commit_record_proves_absence(&Err(Error::from(kind))),
            "`{kind:?}` is a stat the filesystem declined to answer, not an absence"
        );
    }
    assert!(
        !ownership::commit_record_proves_absence(&fs::symlink_metadata(&husk.private)),
        "a successful stat is a record that is present"
    );

    // (2) The wiring: the predicate is what conjunct 12 consults, so the
    // reachable shapes go through the real proof.
    assert!(
        matches!(husk.prove(), PrivateHalfOwnership::Proven(_)),
        "an absent record (a real `NotFound`) still proves"
    );
    write(&husk.private.join(COMMIT_RECORD), b"{}");
    assert!(
        matches!(
            husk.prove(),
            PrivateHalfOwnership::Retained(RetainReason::PossiblyCommitted)
        ),
        "a present record retains"
    );

    // (3) And the two paths into the boundary now agree on the third shape.
    assert!(
        !CommitRecordPresence::Unknown("io".to_owned()).permits_deletion(),
        "the creator's stat refuses an unanswerable filesystem"
    );
}

#[test]
fn the_proof_token_names_the_half_it_authorises_and_nothing_else() {
    let husk = BoundHusk::new("tokentarget");
    husk.publish();
    let PrivateHalfOwnership::Proven(token) = husk.prove() else {
        panic!("the bound husk proves");
    };
    assert_eq!(token.public_dir(), husk.public());
    assert_eq!(token.run_id(), BOUND_RUN);
    assert!(token.target().ends_with(BOUND_RUN));

    // And spending it removes exactly that half, leaving the public one.
    remove_private_husk(token, &mut NoHooks).expect("the token authorises this deletion");
    assert!(!husk.private.exists(), "the private half is gone");
    assert!(husk.public().is_dir(), "the public half is a separate step");
}

#[test]
fn the_public_husk_is_removed_with_its_marker_last() {
    // `startup_census`: "the public directory is removed with the marker
    // last … so a kill mid-census leaves a husk the next census
    // completes". A marker removed first would leave a marker-less husk
    // with content, which the next census retains rather than finishes.
    let husk = BoundHusk::new("publiclast");
    husk.publish();
    write(&lock_file(&husk.public()), b"");
    write(&husk.public().join(PLAN), b"{}");

    struct MarkerWatcher {
        public: PathBuf,
        marker_present_at_after: bool,
        others_gone_at_after: bool,
    }
    impl RunDirHooks for MarkerWatcher {
        fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            if site == EffectSiteId::RunDir(RunDirSite::RemovePublicHusk)
                && phase == HookPhase::After
            {
                self.marker_present_at_after = self.public.join(MARKER).exists();
                self.others_gone_at_after = !self.public.join(PLAN).exists();
            }
            Injection::Proceed
        }
    }

    // The `After` hook runs once the directory is gone, so the ordering is
    // observed by killing the removal partway instead: a kill before the
    // marker's own unlink must leave the marker there.
    let mut watcher = MarkerWatcher {
        public: husk.public(),
        marker_present_at_after: false,
        others_gone_at_after: false,
    };
    remove_public_husk(&husk.public(), &mut watcher).expect("remove");
    assert!(!husk.public().exists(), "the public half is gone");
    assert!(
        !watcher.marker_present_at_after && watcher.others_gone_at_after,
        "the whole directory is gone by the after phase"
    );
}

/// The marker really is removed **last**, observed by interrupting the
/// removal (`PR5-RUNDIR-065`).
///
/// `startup_census`: "the public directory is removed with the marker last
/// (`RunDir.RemovePublicHusk`), **so a kill mid-census leaves a husk the
/// next census completes**". The clause after the comma is the whole point
/// of the ordering, and the test above cannot see it — its `After` hook
/// runs once the directory is already gone, so both observations are the
/// same under either order. Its own comment says what would work ("a kill
/// before the marker's own unlink must leave the marker there") and it does
/// not do it.
///
/// The interruption is a **real** failed removal rather than an injection,
/// because there is no injectable coordinate inside the loop and inventing
/// one would mean a new point in a frozen enum. `zz-blocked` sorts after
/// `plan.json`, so the loop provably got partway: an earlier entry is gone
/// and a later one failed.
///
/// Unix only. The fixture needs a removal that fails, and file permissions
/// are how one is built without privilege; a process running as root would
/// defeat them, which is why the precondition is asserted rather than
/// assumed — this fails loudly there rather than passing vacuously.
#[cfg(unix)]
#[test]
fn a_public_husk_removal_that_fails_partway_leaves_the_marker_that_locates_it() {
    use std::os::unix::fs::PermissionsExt as _;

    let husk = BoundHusk::new("publiclast-interrupted");
    husk.publish();
    let public = husk.public();
    write(&public.join(PLAN), b"{}");
    let blocked = public.join("zz-blocked");
    write(
        &blocked.join("inside.txt"),
        b"content the removal cannot reach",
    );
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o500)).expect("seal it");
    assert!(
        fs::remove_dir_all(&blocked).is_err(),
        "this fixture needs a removal that fails, and here one does not — a process with              the privilege to ignore the permission bits cannot measure this"
    );
    assert!(public.join(MARKER).is_file(), "the husk has its marker");

    let error = remove_public_husk(&public, &mut NoHooks)
        .expect_err("the removal cannot finish, so it returns the failure");

    assert!(
        public.join(MARKER).is_file(),
        "the marker survived the failure and still locates this husk for the next              census: {error}"
    );
    assert!(
        !public.join(PLAN).exists(),
        "and the loop really got partway — an earlier entry was removed"
    );
    assert!(
        public.exists(),
        "the public directory itself is still there"
    );

    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).expect("unseal");
    // And once the obstruction is gone the same call finishes the job,
    // which is what "the next census completes" means.
    remove_public_husk(&public, &mut NoHooks).expect("the next census completes it");
    assert!(!public.exists(), "including the marker and the directory");
}

/// `SWEEP-CLASSIFY-009`: a committed run whose directory could not be read
/// is **not** an empty one, and its public half is still there afterwards.
///
/// **This is a unit witness of the fold, not of the census.** It calls
/// classify, prove and remove directly, so it never reaches `scan` or
/// `is_running`; the census-level sequence — including which failures get
/// past those gates and which do not — is
/// `engine::topology::startup::tests::
/// the_census_refuses_to_reclaim_a_committed_run_whose_listing_it_cannot_read`,
/// and that is where the production claim lives. Pass 1 of this pull request's
/// review found the earlier version of this comment claiming an `EMFILE`
/// sequence that `is_running` gates one layer up, and it was right.
///
/// The fold itself, in the order it happens: the marker read at conjunct 1 and
/// the listing under it both fail, `read_dir_names` answered `[]` for the
/// failed listing, `[]` is `unbound_shape`'s `Bare` arm, so the proof answers
/// `NothingBound(Bare)` and the plan is `ReclaimPublicOnly` — which carries no
/// commit-record check anywhere on its path. Both call sites do the same thing
/// with that answer (`engine::topology::startup::apply`'s `ReclaimPublicOnly`
/// arm and `create.rs`'s `stat_after_error`): they call `remove_public_husk`,
/// which lists the directory a **second** time, once the failure has cleared,
/// and removes what it finds.
///
/// So the fixture is that sequence and nothing else: the answer is taken
/// while the directory cannot be read, the permissions go back before the
/// reclaim, and the reclaim runs exactly when the answer licenses it. The
/// control above it is the same directory, readable: it is retained as a
/// marker-less husk carrying content, so readability is the only difference
/// between the two runs.
///
/// Unix only, and the mode bits are asserted rather than assumed: a process
/// with the privilege to ignore them measures nothing, and this fails there
/// rather than passing vacuously.
#[cfg(unix)]
#[test]
fn a_committed_run_the_census_could_not_read_is_not_reclaimed() {
    use std::os::unix::fs::PermissionsExt as _;

    let husk = BoundHusk::new("unreadable-committed");
    husk.publish();
    let public = husk.public();
    // A committed run in its ordinary end state: the log has its first line
    // and the creator's marker is gone.
    fs::remove_file(public.join(MARKER)).expect("the run committed, so the marker went");
    write(
        &public.join(EVENT_LOG),
        format!("{}\n", committed_line(BOUND_RUN, 4)).as_bytes(),
    );
    assert_eq!(
        classify_run_dir(&public),
        RunDirClass::Committed,
        "the fixture must be a committed run, or this measures nothing"
    );

    // The control. Readable, this directory is retained: the listing holds
    // `events.jsonl`, which is content no marker binds.
    match husk.prove() {
        PrivateHalfOwnership::Retained(reason) => assert_eq!(
            reason.kind(),
            "markerless-with-content",
            "readable, it is content with no marker: {reason}"
        ),
        other => panic!("readable, this husk is retained, not {other:?}"),
    }

    // The transient. Everything that reads this directory fails at once.
    fs::set_permissions(&public, fs::Permissions::from_mode(0o000)).expect("close it");
    assert!(
        fs::read_dir(&public).is_err(),
        "this fixture needs a listing that fails, and here one does not — a process with \
         the privilege to ignore the permission bits cannot measure this"
    );
    assert_eq!(
        classify_run_dir(&public),
        RunDirClass::Husk,
        "the classifier's own open fails in the same moment, which is what makes the \
         census reach the proof at all"
    );
    let answer = husk.prove();
    // And it clears, before anything is deleted — which is the whole point:
    // the second listing succeeds.
    fs::set_permissions(&public, fs::Permissions::from_mode(0o700)).expect("open it again");

    match answer {
        // What both call sites do with a `NothingBound`, done here rather
        // than described: the public half is reclaimed, no commit record
        // consulted. Before the fix this arm ran and took `events.jsonl`
        // with it.
        PrivateHalfOwnership::NothingBound(shape) => {
            let reclaimed = remove_public_husk(&public, &mut NoHooks);
            panic!(
                "an unreadable listing answered {shape:?}, the reclaiming answer; the \
                 reclaim it licenses returned {reclaimed:?} and the committed log is {}",
                if public.join(EVENT_LOG).is_file() {
                    "still there"
                } else {
                    "GONE"
                }
            );
        }
        PrivateHalfOwnership::Retained(reason) => {
            assert_eq!(reason.kind(), "listing-unreadable", "{reason}");
            assert!(
                reason.to_string().contains(&public.display().to_string()),
                "the operator is told which directory did not answer: {reason}"
            );
        }
        PrivateHalfOwnership::Proven(_) => {
            panic!("a directory that could not be read proved nothing")
        }
    }

    assert!(
        public.join(EVENT_LOG).is_file(),
        "the committed run's log is still here"
    );
    assert_eq!(
        classify_run_dir(&public),
        RunDirClass::Committed,
        "and the run is still a committed run"
    );
}

/// A name the listing cannot render as UTF-8 is still removed **by its real
/// name**, rather than missed.
///
/// `read_dir_names` mapped every entry through `to_string_lossy()` and
/// `remove_public_husk` joined the result back into a path, so an entry named
/// with the bytes `x` + `0xff` — a perfectly ordinary Unix filename — was
/// listed as `x` + `U+FFFD` and the removal targeted a **different file**. It
/// returned `NotFound`, left the real entry and the marker behind, and every
/// later census repeated it. A pull request about not deleting the wrong thing
/// on bad evidence cannot itself name the wrong thing.
///
/// Unix only, and stated rather than skipped: a filename that is not valid
/// UTF-8 is constructible on Unix through `OsStrExt` and not on Windows, whose
/// names are UTF-16. The leg that proves this is `test (ubuntu-latest)` and
/// `test (macos-latest)`; the Windows guest compiles the change but does not
/// witness this property.
#[cfg(unix)]
#[test]
fn an_entry_whose_name_is_not_utf8_is_removed_by_its_real_name() {
    use std::os::unix::ffi::OsStrExt as _;

    let husk = BoundHusk::new("lossy-miss");
    husk.publish();
    let public = husk.public();
    let raw = OsStr::from_bytes(b"x\xff");
    write(&public.join(raw), b"the entry the removal must target");
    assert!(
        public.join(raw).is_file(),
        "the fixture needs an entry whose name is not valid UTF-8"
    );

    remove_public_husk(&public, &mut NoHooks)
        .expect("the removal names every entry exactly as the filesystem spells it");

    assert!(
        !public.exists(),
        "the husk is gone: the entry that could not be rendered was removed rather than missed"
    );
}

/// Two entries whose lossy renderings are equal stay **two entries**, so the
/// removal cannot take a valid neighbour in a mangled entry's place.
///
/// This is the second half of the same defect and the dangerous one. A
/// directory holding both `x` + `0xff` and a genuine `x` + `U+FFFD` listed one
/// name twice under `to_string_lossy()`: the removal deleted the valid
/// neighbour, then failed `NotFound` on the second copy of that name, and the
/// entry it was actually asked to remove survived. So the wrong file was
/// deleted — the failure mode this pull request exists to close, reached
/// through the name rather than through the listing.
///
/// The listing is asserted directly as well as through the removal, because
/// the removal alone cannot say *which* of the two it took.
///
/// Unix only, for the reason given above.
#[cfg(unix)]
#[test]
fn two_entries_with_one_lossy_rendering_stay_two_entries() {
    use std::os::unix::ffi::OsStrExt as _;

    let husk = BoundHusk::new("lossy-collision");
    husk.publish();
    let public = husk.public();
    let raw = OsStr::from_bytes(b"x\xff");
    let neighbour = OsStr::new("x\u{FFFD}");
    assert_ne!(
        raw, neighbour,
        "the fixture needs two genuinely different names"
    );
    assert_eq!(
        raw.to_string_lossy(),
        neighbour.to_string_lossy(),
        "whose lossy renderings are the same, or this witnesses nothing"
    );
    write(&public.join(raw), b"the entry a lossy listing loses");
    write(
        &public.join(neighbour),
        b"the valid neighbour it was removed in place of",
    );

    let listed = read_dir_names(&public).expect("the husk lists");
    let distinct: std::collections::BTreeSet<&std::ffi::OsString> = listed.iter().collect();
    assert_eq!(
        distinct.len(),
        listed.len(),
        "two entries collapsed to one name in the listing: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .all(|name| public.join(name).symlink_metadata().is_ok()),
        "every name the listing gives must name an entry that is there: {listed:?}"
    );

    remove_public_husk(&public, &mut NoHooks).expect("both entries are removed, each by its name");
    assert!(!public.exists(), "and the husk is gone");
}

/// A private target the census cannot ask about is not a target that is
/// gone, and the marker that locates it survives.
///
/// `TargetAbsent` is a reclaiming answer, and its reclaim deletes the public
/// directory with `.creating` inside it. That marker is the private half's
/// only locator, so reading a stat that failed as "the target is gone"
/// orphans a private half that is still there — permanently, because
/// `create.rs` says "a private half no marker names is one no census, no
/// `status` and no deferred prune can ever reach again".
///
/// The door is a permission on a **parent component** of the recorded
/// locator, which is what an operator meets: the private root's own
/// directory bits, an `EACCES` from a mount, a directory another process is
/// re-creating. `lstat(2)` takes no file descriptor, so this is a different
/// door from `SWEEP-CLASSIFY-009`'s descriptor exhaustion and the listing's
/// repair does not close it; the grid case above pins the classification on
/// every platform, and this pins the shape that actually happens.
///
/// Unix only, and the precondition is asserted rather than assumed: a
/// process privileged enough to ignore the mode bits fails here rather than
/// passing vacuously.
#[cfg(unix)]
#[test]
fn a_private_target_that_cannot_be_stat_ed_is_not_a_target_that_is_gone() {
    use std::os::unix::fs::PermissionsExt as _;

    let husk = BoundHusk::new("target-unstattable");
    husk.publish();
    let public = husk.public();
    let parent = husk
        .private
        .parent()
        .expect("the runs directory")
        .to_path_buf();

    // The control: readable, this husk proves and is reclaimable in full.
    match husk.prove() {
        PrivateHalfOwnership::Proven(token) => {
            assert_eq!(
                token.run_id(),
                BOUND_RUN,
                "the control must be the happy path"
            )
        }
        other => panic!("the control must prove, not {other:?}"),
    }

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).expect("close the parent");
    let stat = fs::symlink_metadata(&husk.private);
    let readable_again = || {
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("open the parent")
    };
    if stat.is_ok() {
        readable_again();
        panic!(
            "this fixture needs a stat that fails, and here one does not — a process with \
             the privilege to ignore the permission bits cannot measure this"
        );
    }

    let answer = husk.prove();
    readable_again();

    match answer {
        // What the census does with a `NothingBound`: the public half goes,
        // and `.creating` goes with it. Before the fix this arm ran.
        PrivateHalfOwnership::NothingBound(shape) => {
            let reclaimed = remove_public_husk(&public, &mut NoHooks);
            panic!(
                "a stat that could not answer answered {shape:?}, a reclaiming answer; the \
                 reclaim it licenses returned {reclaimed:?} and the marker that locates the \
                 private half is {}",
                if public.join(MARKER).is_file() {
                    "still there"
                } else {
                    "GONE"
                }
            );
        }
        PrivateHalfOwnership::Retained(reason) => {
            assert_eq!(reason.kind(), "target-undecidable", "{reason}");
            assert!(
                reason
                    .to_string()
                    .contains(&husk.private.display().to_string()),
                "the operator is told which target could not be asked about: {reason}"
            );
        }
        PrivateHalfOwnership::Proven(_) => {
            panic!("a target that could not be stat-ed proved nothing")
        }
    }

    assert!(
        public.join(MARKER).is_file(),
        "the marker that locates the private half is still here"
    );
    assert!(
        husk.private.join(OWNER_RECORD).is_file(),
        "and so is the private half it names"
    );
}

/// The removal's **own** listing, which is the second observation and the
/// one that deletes.
///
/// `remove_public_husk` ran its loop over `read_dir_names`' silent
/// `Vec::new()`, so a directory it could not list was one it removed nothing
/// from — and then unlinked the marker anyway and failed on the non-empty
/// directory. That leaves a husk carrying content whose private half no
/// marker names any more, and `create.rs` says what that costs: "a private
/// half no marker names is one no census, no `status` and no deferred prune
/// can ever reach again".
///
/// Mode `0o300` is the shape descriptor exhaustion has, built without
/// privilege: search and write, no read. `read_dir` fails, and the unlink of
/// the marker inside it would succeed — a listing needs a descriptor and an
/// unlink does not.
#[cfg(unix)]
#[test]
fn a_public_removal_whose_listing_does_not_answer_removes_nothing() {
    use std::os::unix::fs::PermissionsExt as _;

    let husk = BoundHusk::new("publichusk-unlistable");
    husk.publish();
    let public = husk.public();
    write(&public.join(PLAN), b"{}");

    fs::set_permissions(&public, fs::Permissions::from_mode(0o300)).expect("no read bit");
    assert!(
        fs::read_dir(&public).is_err(),
        "this fixture needs a listing that fails, and here one does not — a process with \
         the privilege to ignore the permission bits cannot measure this"
    );

    let error = remove_public_husk(&public, &mut NoHooks)
        .expect_err("a removal that cannot list what it is removing refuses");

    fs::set_permissions(&public, fs::Permissions::from_mode(0o700)).expect("open it again");
    assert!(
        public.join(MARKER).is_file(),
        "the marker that locates the private half survived: {error}"
    );
    assert!(
        public.join(PLAN).is_file(),
        "and so did the content the listing never named: {error}"
    );
    assert!(
        error.to_string().contains(&public.display().to_string()),
        "the failure names the directory it could not list: {error}"
    );
}

/// A public half whose only content is `.creating.tmp` is **removed**, not
/// retained.
///
/// The shape and the removal are each exercised and never composed. The
/// grid's "staged marker only" case asserts `NothingBound(StagedMarkerOnly)`
/// and stops at the classification; every fixture that reaches
/// `remove_public_husk` drives a husk carrying a **published** marker plus
/// other content. So a removal that skipped the staging file the way it
/// skips the published marker left the directory non-empty and its final
/// `remove_dir` failing, with nothing in the suite to observe it — measured
/// surviving on Linux and on the Windows guest.
///
/// `startup_census` (i) reclaims "a bare directory or one holding only a
/// staged `.creating.tmp`", and the census reaching it is the whole
/// obligation: retained-as-markerless-content is the outcome this shape
/// must never get.
///
/// The retry half **reconstructs** the state an interrupted first pass
/// leaves — the other content gone, the staging file and the directory
/// still there — rather than interrupting a real one. Building a genuine
/// mid-loop failure needs a removal that fails, which is a permission
/// fixture and therefore Unix-only
/// (`a_public_husk_removal_that_fails_partway_leaves_the_marker_that_locates_it`
/// is exactly that and is `#[cfg(unix)]`). What convergence needs is that
/// the *state* is reached and finished, and this reaches it on both
/// platforms.
#[test]
fn probe_a_staged_marker_only_public_husk_is_removed() {
    let root = scratch("probe-stagedonly");

    let public = root.join("runs").join("01STAGEDONLY");
    fs::create_dir_all(&public).expect("public directory");
    write(&public.join(MARKER_STAGED), b"{}");
    remove_public_husk(&public, &mut NoHooks).expect("the census removes a staged-marker husk");
    assert!(
        !public.exists(),
        "the public directory itself is gone, staging file and all"
    );

    // And it converges across an interrupted first pass.
    let retried = root.join("runs").join("01RETRY");
    fs::create_dir_all(&retried).expect("public directory");
    write(&retried.join(MARKER_STAGED), b"{}");
    write(&retried.join(PLAN), b"{}");
    fs::remove_file(retried.join(PLAN)).expect("the interrupted pass got this far");
    remove_public_husk(&retried, &mut NoHooks).expect("the retry converges");
    assert!(!retried.exists(), "the next census finishes the job");
}

/// P0 creates the **public** run directory and nothing else
/// (`PR5-RUNDIR-036`).
///
/// `run_creation` orders "P0 create the public run directory
/// (`RunDir.CreatePublicDir`)" before "P3 create the private half at the
/// recorded locator", and the private half exists so that no agent-authored
/// byte is reachable from the workspace. Implementing P0 by calling the
/// legacy `RunPaths::create()` — which builds both halves and both
/// skeletons — satisfied every site-coverage assertion in this file,
/// because none of them ever looked at what was on disk at a phase.
#[test]
fn p0_creates_the_public_directory_and_nothing_private() {
    let root = scratch("p0-only");
    let paths = paths_in(&root, "01P0ONLY");
    let public = paths.public.clone();
    let private = paths.private.clone();

    create_public_dir(&public, &mut NoHooks).expect("P0");

    assert!(public.is_dir(), "P0 created the public run directory");
    assert_eq!(
        read_dir_names(&public).expect("the public directory lists"),
        Vec::<std::ffi::OsString>::new(),
        "and it is bare: no skeleton, no marker, no private half beneath it"
    );
    assert!(
        !private.exists(),
        "the private half is P3's, at the recorded locator, and does not exist yet"
    );
}

/// The owner record is the **first content** of a private half
/// (`PR5-RUNDIR-044`).
///
/// `side_effect_vs_event_ordering` says exactly that, and until now it was
/// asserted by nothing: no test read the private half's directory listing
/// at any point in the publication sequence, so moving the five skeleton
/// directories into `create_private_dir`'s own funnel body — where they
/// exist before `owner.json` is even staged — changed nothing observable.
#[test]
fn the_owner_record_is_the_first_content_of_a_private_half() {
    let root = scratch("owner-first");
    let private = root.join("private").join("runs").join("01OWNERFIRST");
    let owner = OwnerRecord {
        run_id: "01OWNERFIRST".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        public_dir: root.join("public").to_string_lossy().into_owned(),
        incarnation: "01INC".to_owned(),
        runner: crate::runner::policy::host_policy(),
    };

    create_private_dir(&private, &mut NoHooks).expect("P3");
    assert_eq!(
        read_dir_names(&private).expect("the private half lists"),
        Vec::<std::ffi::OsString>::new(),
        "immediately after P3 the private half is empty"
    );

    stage_owner_record(&private, &owner, &mut NoHooks).expect("P3a");
    assert_eq!(
        read_dir_names(&private).expect("the private half lists"),
        vec![std::ffi::OsString::from(OWNER_RECORD_STAGED)],
        "the staged owner record is the only thing in it"
    );

    publish_owner_record(&private, &mut NoHooks).expect("P3b");
    assert_eq!(
        read_dir_names(&private).expect("the private half lists"),
        vec![std::ffi::OsString::from(OWNER_RECORD)],
        "and after publication the owner record is the only content there has ever been"
    );
}

// =======================================================================
// The funnel
// =======================================================================

/// Records what the funnels reached, and answers with whatever was armed.
#[derive(Debug, Default)]
struct Observer {
    reached: Vec<(String, HookPhase)>,
    armed: Vec<(EffectSiteId, HookPhase, Injection)>,
}

impl Observer {
    fn arm(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
        self.armed.push((site, phase, injection));
    }

    fn sites(&self) -> Vec<String> {
        let mut sites: Vec<String> = self.reached.iter().map(|(site, _)| site.clone()).collect();
        sites.sort_unstable();
        sites.dedup();
        sites
    }

    fn phases_of(&self, site: EffectSiteId) -> Vec<HookPhase> {
        let name = site.to_string();
        let mut phases: Vec<HookPhase> = self
            .reached
            .iter()
            .filter(|(seen, _)| *seen == name)
            .map(|(_, phase)| *phase)
            .collect();
        phases.dedup();
        phases
    }
}

impl RunDirHooks for Observer {
    fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        self.reached.push((site.to_string(), phase));
        self.armed
            .iter()
            .find(|(armed, at, _)| *armed == site && *at == phase)
            .map_or(Injection::Proceed, |(_, _, injection)| *injection)
    }
}

/// Every site of the three groups this module funnels, from the frozen
/// inventory's own `ALL` slices.
fn sites_this_module_owns() -> Vec<String> {
    let mut names: Vec<String> = RunDirSite::ALL
        .iter()
        .map(|site| EffectSiteId::RunDir(*site).to_string())
        .chain(
            AnswerSite::ALL
                .iter()
                .map(|site| EffectSiteId::Answer(*site).to_string()),
        )
        .chain(
            LockSite::ALL
                .iter()
                .map(|site| EffectSiteId::Lock(*site).to_string()),
        )
        .collect();
    names.sort_unstable();
    names
}

fn commit_record_of(husk: &BoundHusk) -> CommitRecord {
    CommitRecord {
        run_id: BOUND_RUN.to_owned(),
        repo_key: husk.repo_key.as_str().to_owned(),
        public_dir: husk.owner.public_dir.clone(),
        incarnation: BOUND_INCARNATION.to_owned(),
        run_started_sha256: run_started_sha256(committed_line(BOUND_RUN, 4).as_bytes()),
    }
}

/// Every atomic publication's **durability sequence**, read out of the
/// funnel's own ledger (`PR5-RUNDIR-057`).
///
/// `run_creation` spells each of the three the same way — "write
/// `<name>.tmp`, **fsync**, rename, **fsync the directory**" — and until
/// this lane had a ledger, two of those four steps were not observables at
/// all. Deleting `stage_json`'s `file.sync_all()`, which is the staging
/// half of the marker, the owner record *and* the commit record, left the
/// entire suite green: every consumer checks the *outcome* of a publication
/// (the staged name is gone, the published name holds the right JSON, the
/// census parses it) and an unsynced file is byte-for-byte a synced one on
/// a machine that does not lose power.
///
/// The ledger's length is the filesystem's own answer rather than a number
/// the funnel carried along, so a sync that reported a length while the
/// file held something else would fail here rather than agree with itself.
#[test]
fn every_atomic_publication_syncs_the_staged_file_then_renames_then_syncs_its_directory() {
    let root = scratch("durability");
    let public = root.join("public");
    let private = root.join("private");
    create_dir(&public).expect("public");
    create_dir(&private).expect("private");
    let policy = crate::runner::policy::host_policy();
    let marker = CreatingMarker {
        run_id: "01LEDGER".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        private_dir: private.to_string_lossy().into_owned(),
        incarnation: "01INC".to_owned(),
        pid: std::process::id(),
        runner_policy_sha256: runner_policy_sha256(&policy),
    };
    let owner = OwnerRecord {
        run_id: "01LEDGER".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        public_dir: public.to_string_lossy().into_owned(),
        incarnation: "01INC".to_owned(),
        runner: policy,
    };
    let commit = CommitRecord {
        run_id: "01LEDGER".to_owned(),
        repo_key: "0123456789abcdef".to_owned(),
        public_dir: public.to_string_lossy().into_owned(),
        incarnation: "01INC".to_owned(),
        run_started_sha256: run_started_sha256(b"{}\n"),
    };

    let mut hooks = HarnessHooks::default().recording_durability();
    let ledger = hooks.ledger();
    // The ledger is written *beside* the syscall by the same function, so on
    // its own it certifies itself (`PR5-CONF-012`): with `sync_all` replaced
    // by `Ok(())`, every assertion below still passed. `barriers_performed`
    // counts entries into `util::fsync_file`/`fsync_dir`, so the ledger's
    // claim can be checked against something that is not the ledger.
    //
    // The two axes are the *record* and the *call*. Every assertion below
    // holds the call constant — it is assumed to have happened — and reads
    // the record; this reads the call and holds the record constant. The
    // counter is process-wide and the suite is threaded, so the assertion is
    // a **lower bound on the delta**, which is the strongest thing a shared
    // counter can support and is still zero if the barrier is never entered.
    let barriers_before = util::barriers_performed();
    let publications: Vec<(&str, PathBuf, &str, &str)> = vec![
        ("marker", public.clone(), MARKER_STAGED, MARKER),
        (
            "owner record",
            private.clone(),
            OWNER_RECORD_STAGED,
            OWNER_RECORD,
        ),
        (
            "commit record",
            private.clone(),
            COMMIT_RECORD_STAGED,
            COMMIT_RECORD,
        ),
    ];
    for (which, dir, staged_name, published_name) in publications {
        ledger.clear();
        match which {
            "marker" => {
                stage_marker(&public, &marker, &mut hooks).expect("P1a");
                publish_marker(&public, &mut hooks).expect("P1b");
            }
            "owner record" => {
                stage_owner_record(&private, &owner, &mut hooks).expect("P3a");
                publish_owner_record(&private, &mut hooks).expect("P3b");
            }
            _ => {
                stage_commit_record(&private, &commit, &mut hooks).expect("P5a");
                publish_commit_record(&private, &mut hooks).expect("P5b");
            }
        }

        let records = ledger.records();
        // One expectation for every platform (`PR5-CONF-013`). This used to
        // fork on `cfg!(unix)` because `sync_dir` was a documented no-op on
        // Windows; `run_creation`'s "fsync the directory" carries no
        // platform exception, and now neither does this.
        let expected: Vec<DurableStep> = vec![
            DurableStep::SyncedFile,
            DurableStep::Renamed,
            DurableStep::SyncedDirectory,
        ];
        assert_eq!(
            ledger.steps(),
            expected,
            "{which}: the durability sequence run_creation names, in order"
        );
        assert_eq!(
            records[0].path,
            dir.join(staged_name),
            "{which}: the sync is of the STAGED file, before it has its published name"
        );
        let published_len = fs::metadata(dir.join(published_name))
            .expect("the published record")
            .len();
        assert!(published_len > 0, "{which}: the record has bytes at all");
        assert_eq!(
            records[0].len, published_len,
            "{which}: the whole staged file was synced, not a prefix of it"
        );
        assert_eq!(
            records[1].path,
            dir.join(published_name),
            "{which}: the rename lands on the published name"
        );
        assert_eq!(
            records[2].path, dir,
            "{which}: the directory sync is of the directory the rename changed"
        );
    }

    // Three publications, each recording one file sync and one directory
    // sync: six ledger entries that each claim a barrier was performed.
    let claimed = 6;
    let performed = util::barriers_performed().saturating_sub(barriers_before);
    assert!(
        performed >= claimed,
        "the ledger recorded {claimed} durability barriers and only {performed} \
             were entered; a ledger that certifies the function it is written by \
             cannot tell the two apart (PR5-CONF-012)"
    );
}

#[test]
fn every_site_this_module_owns_is_reached_through_a_funnel_in_both_phases() {
    // Enumerated from `RunDirSite::ALL`, `AnswerSite::ALL` and
    // `LockSite::ALL` rather than from a list of what this file happens to
    // call, so a site the frozen inventory declares and no funnel names
    // fails here rather than being quietly absent from `effect_sites.json`.
    let husk = BoundHusk::new("sitecoverage");
    let mut hooks = Observer::default();
    let public = husk.public();

    create_public_dir(&public, &mut hooks).expect("P0");
    stage_marker(&public, &husk.marker, &mut hooks).expect("P1a");
    publish_marker(&public, &mut hooks).expect("P1b");
    create_private_dir(&husk.private, &mut hooks).expect("P3");
    stage_owner_record(&husk.private, &husk.owner, &mut hooks).expect("P3a");
    publish_owner_record(&husk.private, &mut hooks).expect("P3b");
    write_plan(&public, b"{\"tasks\":[]}", &mut hooks).expect("P5");
    write_report(
        &public,
        &serde_json::json!({"outcome": "parked"}),
        &mut hooks,
    )
    .expect("report");
    let questions = public.join("questions");
    fs::create_dir_all(&questions).expect("questions");
    write_question_payload(&questions, "q-1", &serde_json::json!({}), &mut hooks)
        .expect("question payload");
    let answers = public.join("answers");
    fs::create_dir_all(&answers).expect("answers");
    stage_answer(&answers, "q-1", &serde_json::json!({}), &mut hooks).expect("stage answer");
    publish_answer(&answers, "q-1", &mut hooks).expect("publish answer");
    ingest_answer(&answers, "q-1", &mut hooks).expect("ingest answer");

    // The commit record goes to a private half of its own, so publishing
    // it does not make the husk below unprovable.
    let committed_half = husk
        .root
        .join("private")
        .join("runs")
        .join("01COMMITTEDHALF");
    create_private_dir(&committed_half, &mut hooks).expect("second private half");
    stage_commit_record(&committed_half, &commit_record_of(&husk), &mut hooks).expect("P5a");
    publish_commit_record(&committed_half, &mut hooks).expect("P5b");

    let git_dir = husk.root.join("git-dir");
    fs::create_dir_all(&git_dir).expect("git dir");
    let lease = WorktreeLock::acquire_in_hooked(&husk.repo, &git_dir, &mut hooks)
        .expect("the worktree lease");
    let run_lock = RunLock::acquire_hooked(&public, &mut hooks).expect("the run lock");
    run_lock.release(&mut hooks);
    drop(lease);

    remove_marker(&public, &mut hooks).expect("P7");
    // The marker is gone, so re-publish it for the proof, then spend the
    // token on the half it names.
    stage_marker(&public, &husk.marker, &mut hooks).expect("re-stage");
    publish_marker(&public, &mut hooks).expect("re-publish");
    let PrivateHalfOwnership::Proven(token) = husk.prove() else {
        panic!("the bound husk proves");
    };
    remove_private_husk(token, &mut hooks).expect("private half");
    remove_public_husk(&public, &mut hooks).expect("public half");

    assert_eq!(
        hooks.sites(),
        sites_this_module_owns(),
        "every declared site, and no site this module does not own"
    );
    for name in sites_this_module_owns() {
        let site: EffectSiteId = name.clone().try_into().expect("a declared site");
        assert_eq!(
            hooks.phases_of(site).first(),
            Some(&HookPhase::Before),
            "`{name}` must hook Before its primitive"
        );
        assert!(
            hooks.phases_of(site).contains(&HookPhase::After),
            "`{name}` must hook After it"
        );
    }
}

#[test]
fn the_post_error_stat_helper_stats_rather_than_reading_the_error() {
    // The two cases `run_creation` separates — "a P5b error after which
    // the record is absent" and "a P5b error after which the record is
    // present" — return the *same* error value, because the error-return
    // mode returns `Err` after performing the primitive. A helper that
    // inferred absence from an error would delete a private half that had
    // already crossed the deletion boundary.
    let husk = BoundHusk::new("posterror");
    husk.publish();
    let record = commit_record_of(&husk);
    let site = EffectSiteId::RunDir(RunDirSite::PublishCommitRecord);

    // (1) the rename happened, then the funnel returned Err.
    stage_commit_record(&husk.private, &record, &mut NoHooks).expect("stage");
    let mut after = Observer::default();
    after.arm(site, HookPhase::After, Injection::Error);
    let error = publish_commit_record(&husk.private, &mut after).expect_err("injected");
    assert!(
        error.to_string().contains("RunDir.PublishCommitRecord"),
        "the error names the point reached: {error}"
    );
    assert!(
        husk.private.join(COMMIT_RECORD).is_file(),
        "the record is there"
    );
    assert_eq!(
        commit_record_after_error(&husk.private),
        CommitRecordPresence::Present
    );
    assert!(
        !commit_record_after_error(&husk.private).permits_deletion(),
        "from the moment committed.json exists the creator deletes nothing"
    );
    // And the census agrees with the creator about the same bytes.
    assert!(matches!(
        husk.prove(),
        PrivateHalfOwnership::Retained(RetainReason::PossiblyCommitted)
    ));

    // (2) the same error, returned before the rename.
    fs::remove_file(husk.private.join(COMMIT_RECORD)).expect("reset");
    stage_commit_record(&husk.private, &record, &mut NoHooks).expect("stage again");
    let mut before = Observer::default();
    before.arm(site, HookPhase::Before, Injection::Error);
    publish_commit_record(&husk.private, &mut before).expect_err("injected");
    assert_eq!(
        commit_record_after_error(&husk.private),
        CommitRecordPresence::Absent
    );
    assert!(
        commit_record_after_error(&husk.private).permits_deletion(),
        "the creator knows the run never committed and may remove both halves"
    );
    assert!(
        husk.private.join(COMMIT_RECORD_STAGED).is_file(),
        "committed.json.tmp leaves with the private half"
    );
    assert!(
        matches!(husk.prove(), PrivateHalfOwnership::Proven(_)),
        "a staged-only commit record is not a commit record"
    );

    // (3) an unreadable answer is not "absent".
    assert!(!CommitRecordPresence::Unknown("io".to_owned()).permits_deletion());
}

/// The child of [`a_kill_between_stage_and_rename_leaves_only_the_tmp`]:
/// stages one record and dies at the publication site's `Before` phase.
#[test]
#[ignore = "spawned as a subprocess by a_kill_between_stage_and_rename_leaves_only_the_tmp"]
fn publication_kill_child() {
    let dir = PathBuf::from(std::env::var("UPSTROKE_TEST_KILL_DIR").expect("dir"));
    let which = std::env::var("UPSTROKE_TEST_KILL_SITE").expect("site");
    fs::create_dir_all(&dir).expect("dir");
    let policy = crate::runner::policy::host_policy();
    let mut hooks = Observer::default();
    let (site, publish): (RunDirSite, fn(&Path, &mut dyn RunDirHooks) -> _) = match which.as_str() {
        "marker" => {
            let marker = CreatingMarker {
                run_id: "01KILL".to_owned(),
                repo_key: "0123456789abcdef".to_owned(),
                private_dir: dir.to_string_lossy().into_owned(),
                incarnation: "01INC".to_owned(),
                pid: std::process::id(),
                runner_policy_sha256: runner_policy_sha256(&policy),
            };
            stage_marker(&dir, &marker, &mut hooks).expect("stage marker");
            (RunDirSite::PublishMarker, publish_marker)
        }
        "owner" => {
            let owner = OwnerRecord {
                run_id: "01KILL".to_owned(),
                repo_key: "0123456789abcdef".to_owned(),
                public_dir: dir.to_string_lossy().into_owned(),
                incarnation: "01INC".to_owned(),
                runner: policy,
            };
            stage_owner_record(&dir, &owner, &mut hooks).expect("stage owner");
            (RunDirSite::PublishOwnerRecord, publish_owner_record)
        }
        "commit" => {
            let record = CommitRecord {
                run_id: "01KILL".to_owned(),
                repo_key: "0123456789abcdef".to_owned(),
                public_dir: dir.to_string_lossy().into_owned(),
                incarnation: "01INC".to_owned(),
                run_started_sha256: "sha256:00".to_owned(),
            };
            stage_commit_record(&dir, &record, &mut hooks).expect("stage commit");
            (RunDirSite::StageCommitRecord, publish_commit_record)
        }
        other => panic!("unknown site `{other}`"),
    };
    let site = match which.as_str() {
        "commit" => RunDirSite::PublishCommitRecord,
        _ => site,
    };
    hooks.arm(
        EffectSiteId::RunDir(site),
        HookPhase::Before,
        Injection::Kill,
    );
    let _ = publish(&dir, &mut hooks);
    unreachable!("the kill must have taken this process");
}

#[test]
fn a_kill_between_stage_and_rename_leaves_only_the_tmp() {
    // A real process death, not an early return: the claim is what a
    // coordinator that runs *no* cleanup leaves on disk, and the funnel's
    // kill aborts rather than unwinding for exactly that reason.
    let root = scratch("killpublish");
    for (which, staged, published) in [
        ("marker", MARKER_STAGED, MARKER),
        ("owner", OWNER_RECORD_STAGED, OWNER_RECORD),
        ("commit", COMMIT_RECORD_STAGED, COMMIT_RECORD),
    ] {
        let dir = root.join(which);
        let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "rundir::tests::publication_kill_child",
                "--ignored",
                "--nocapture",
            ])
            .env("UPSTROKE_TEST_KILL_DIR", &dir)
            .env("UPSTROKE_TEST_KILL_SITE", which)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("spawn the publishing child");
        assert!(!status.success(), "`{which}`: the child must have died");
        assert!(
            dir.join(staged).is_file(),
            "`{which}`: the staged file survives the kill"
        );
        assert!(
            !dir.join(published).exists(),
            "`{which}`: nothing was published"
        );
    }
}

/// Publication re-points the *name*; it never writes through it.
///
/// The kill test above cannot see this. It kills at `Before`, where neither
/// a rename nor a copy has done anything yet, so it stays green against a
/// `publish` rewritten as copy-then-delete — measured, and the reason this
/// test exists. Copy-then-delete is not atomic: it truncates the
/// destination and then fills it, so a death inside it leaves a *partial*
/// published record where `T-RUNSTART` requires either the old one or the
/// new one. `RunDirSite::sub_effects()` is empty for every site in the
/// frozen inventory, so there is no coordinate to place a fault at inside
/// the primitive, and the discriminator has to be an observable the two
/// implementations differ on *after* a successful publication.
///
/// A hard link is that observable, on both platforms. Point a second name
/// at the destination before publishing: `fs::rename` replaces the
/// directory entry and leaves the linked file's bytes alone, while
/// `fs::copy` opens that same file through the link and overwrites it. So
/// the sentinel's bytes answer "rename or copy?" directly, with no reliance
/// on `st_ino` — which Windows does not expose on stable Rust
/// (`MetadataExt::file_index` is behind `windows_by_handle`).
#[test]
fn publication_replaces_the_name_rather_than_writing_through_it() {
    let root = scratch("publishrename");
    for (which, staged_name, published_name) in [
        ("marker", MARKER_STAGED, MARKER),
        ("owner", OWNER_RECORD_STAGED, OWNER_RECORD),
        ("commit", COMMIT_RECORD_STAGED, COMMIT_RECORD),
    ] {
        let dir = root.join(which);
        fs::create_dir_all(&dir).expect("dir");

        // The bytes that must survive: an unrelated file that happens to
        // share an inode with the publication's destination.
        let sentinel = dir.join("sentinel");
        let sentinel_bytes = b"the linked file is not the publication's business";
        fs::write(&sentinel, sentinel_bytes).expect("sentinel");
        fs::hard_link(&sentinel, dir.join(published_name)).expect("hard link");

        let staged_bytes = b"{\"published\":true}";
        fs::write(dir.join(staged_name), staged_bytes).expect("staged");
        publish(
            &dir.join(staged_name),
            &dir.join(published_name),
            &DurabilityLedger::off(),
        )
        .expect("publish");

        assert_eq!(
            fs::read(dir.join(published_name)).expect("published"),
            staged_bytes,
            "`{which}`: the published name carries the staged bytes"
        );
        assert!(
            !dir.join(staged_name).exists(),
            "`{which}`: the staged name is gone"
        );
        assert_eq!(
            fs::read(&sentinel).expect("sentinel after"),
            sentinel_bytes,
            "`{which}`: publication wrote *through* the destination name \
                 instead of replacing it, so it is a copy rather than a rename \
                 and a death inside it can leave a partial record"
        );
    }
}

// =======================================================================
// R28: a surviving reaper's shared cleanup hold
// =======================================================================

/// A reaper that outlives its coordinator: takes the shared cleanup hold
/// and keeps it until it is killed.
#[cfg(unix)]
#[test]
#[ignore = "spawned as a subprocess by a_surviving_reaper_hold_refuses_the_next_coordinator_until_released"]
fn cleanup_hold_child() {
    use std::os::fd::AsRawFd as _;
    let public = PathBuf::from(std::env::var("UPSTROKE_TEST_CLEANUP_DIR").expect("run dir"));
    let path = cleanup_lock_file(&public);
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .expect("open the cleanup lock");
    // SHARED, which is what R28 is: "a surviving Unix cleanup reaper's
    // **shared** cleanup.lock hold (one per reaper)".
    assert_eq!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) },
        0,
        "the reaper takes its shared hold"
    );
    println!("held");
    std::io::Write::flush(&mut std::io::stdout()).expect("flush");
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(unix)]
#[test]
fn a_surviving_reaper_hold_refuses_the_next_coordinator_until_released() {
    // `PR4-R28-NEXT-COORDINATOR-UNWITNESSED`: two withheld mutations
    // survived the whole suite because no test started a coordinator while
    // a surviving reaper actually held R28. `PR4-WIN-073` turns the
    // would-block branch into continuation; `PR4-WIN-074` replaces the
    // immediate refusal with a loop that waits for the hold and then
    // continues. Both are killed here, and by different assertions.
    //
    // The run is a **husk** on purpose. The run whose reaper is still
    // settling groups is the one that died before its log committed, and
    // `list_runs` no longer returns it — so a lease that scanned the
    // readers' view would leave exactly this hold unobserved.
    let root = scratch("r28witness");
    let repo = root.join("repo");
    let git_dir = root.join("git-dir");
    fs::create_dir_all(&git_dir).expect("git dir");
    let husk_id = "01REAPERHUSK00000000000000";
    let husk = public_dir(&repo, husk_id);
    fs::create_dir_all(&husk).expect("husk");
    assert_eq!(classify_run_dir(&husk), RunDirClass::Husk);
    assert!(
        list_runs(&repo).is_empty(),
        "the reader does not return it, which is the point"
    );

    // Adopted, so the child is terminated, reaped and its reader joined
    // when this scope ends however it ends -- including a panicking
    // assertion between here and the teardown below.
    let mut producer = readiness::Producer::adopt(
        std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "rundir::tests::cleanup_hold_child",
                "--ignored",
                "--nocapture",
            ])
            .env("UPSTROKE_TEST_CLEANUP_DIR", &husk)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn the surviving reaper"),
    );
    // Producer-aware and effectively bounded, at the bound this test
    // already used; see `two_run_ids_cannot_drive_one_worktree_concurrently`
    // for what the loop this replaces could not do.
    producer
        .await_line("held", Duration::from_secs(30))
        .or_fail("the reaper never took its hold");

    assert!(
        observe_cleanup_hold(&husk, &mut NoHooks),
        "R28 is held by a live reaper"
    );

    let started = Instant::now();
    let error = WorktreeLock::acquire_in(&repo, &git_dir)
        .expect_err("a coordinator must not overlap a live reaper");
    let waited = started.elapsed();
    assert!(
        error.to_string().contains("still cleaning agent processes"),
        "{error}"
    );
    assert!(
        error.to_string().contains(husk_id),
        "names the run: {error}"
    );
    // Kills the polling-loop mutation: a lease that waited for the hold
    // to release would only have returned after it was gone.
    assert!(
        observe_cleanup_hold(&husk, &mut NoHooks),
        "the refusal returned while the hold was still held"
    );
    assert!(
        waited < Duration::from_secs(5),
        "refused at once rather than waiting the reaper out: {waited:?}"
    );

    // The other observation point: the exclusive probe at run-lock
    // acquisition, which `resource_accounting` names beside the first.
    let error = RunLock::acquire(&husk).expect_err("the exclusive side is refused");
    assert!(error.to_string().contains("already driving run"), "{error}");

    drop(producer);

    // Released with the reaper, by the OS, without anybody resetting it.
    assert!(
        !observe_cleanup_hold(&husk, &mut NoHooks),
        "the hold is gone"
    );
    let lease = WorktreeLock::acquire_in(&repo, &git_dir).expect("and now the lease is free");
    drop(lease);
    let run = RunLock::acquire(&husk).expect("and so is the run lock");
    drop(run);
}

// =======================================================================
// The test build's scratch trees
// =======================================================================

/// Witness 4 — a reclaim removes the token root and nothing outside it.
///
/// The removal is recursive and it is aimed by the token, so what has to be
/// shown is the *extent*: everything under the root goes, and every byte
/// beside it stays. Both halves are asserted against a snapshot of the
/// whole parent tree rather than against a handful of named paths, so a
/// reclaim that reached one directory too far fails here rather than
/// somewhere downstream.
#[test]
fn a_reclaim_removes_the_token_root_and_nothing_outside_it() {
    let parent = scratch_tree::acquire(&std::env::temp_dir(), "extent").expect("the parent tree");

    // Outside the root under test, at two depths: a sibling directory with
    // content, and a file directly beside the root.
    let sibling = parent.path().join("sibling");
    fs::create_dir(&sibling).expect("sibling");
    fs::write(sibling.join("keep.txt"), b"outside the root").expect("sibling content");
    fs::write(parent.path().join("beside.txt"), b"beside the root").expect("parent content");

    let tree = scratch_tree::acquire(parent.path(), "target").expect("the tree under test");
    let root = tree.path().to_path_buf();
    let name = PathBuf::from(root.file_name().expect("the root has a name"));
    let deep = root.join("a").join("b").join("c");
    fs::create_dir_all(&deep).expect("nested directories");
    fs::write(deep.join("inside.txt"), b"inside the root").expect("nested content");
    fs::write(root.join("inside.txt"), b"inside the root").expect("root content");

    let before = snapshot_tree(parent.path());
    let expected: std::collections::BTreeMap<PathBuf, Vec<u8>> = before
        .iter()
        .filter(|(path, _)| !path.starts_with(&name))
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect();
    assert!(
        expected.len() < before.len(),
        "the token root held no files, so this witness would pass on a reclaim that \
             removed nothing"
    );
    assert!(
        !expected.is_empty(),
        "nothing was outside the root to preserve"
    );

    scratch_tree::remove_scratch_tree(tree.disarm()).expect("the tree is reclaimed");

    assert!(
        scratch_tree::proves_absent(&root),
        "the token root is gone, proved rather than assumed"
    );
    assert_eq!(
        snapshot_tree(parent.path()),
        expected,
        "the reclaim removed something outside its token root, or left something inside it"
    );
}

/// Witness 7 — a scratch tree carrying a published `committed.json` is
/// reclaimed through the scratch funnel, while the ownership proof over the
/// same bytes refuses.
///
/// This is the whole reason there are two tokens. The same directory is
/// looked at by both authorities and they answer differently, correctly:
///
/// * `prove_private_half_ownership` answers
///   [`RetainReason::PossiblyCommitted`]. Conjunct 12 is unmoved and
///   fail-closed, so no run-lifecycle path — census or creator — deletes
///   that private half, ever.
/// * `scratch_tree::remove_scratch_tree` reclaims the tree the fixture was
///   built in, because its authority is not about the contents at all: the
///   root did not exist before `acquire` created it, so the `committed.json`
///   inside it is a record this test published seconds ago rather than a
///   run's deletion boundary.
///
/// Routing the fixture's cleanup through `PrivateHalfProof` instead would
/// require either forging that token or weakening conjunct 12 — and the
/// conjunct-12 tests are precisely the ones that need a fixture in this
/// shape.
#[test]
fn a_scratch_tree_holding_a_committed_record_is_reclaimed_while_the_proof_refuses_it() {
    let tree = scratch_tree::acquire(&std::env::temp_dir(), "committed")
        .expect("the scratch tree the fixture is built in");
    let husk = BoundHusk::at(tree.path().to_path_buf());
    husk.publish();

    let hooks = &mut NoHooks;
    stage_commit_record(&husk.private, &commit_record_of(&husk), hooks).expect("P5a");
    publish_commit_record(&husk.private, hooks).expect("P5b");
    let record = husk.private.join(COMMIT_RECORD);
    assert!(record.is_file(), "the fixture published a commit record");

    // The run-lifecycle authority refuses, and refuses for the boundary's
    // own reason rather than for some incidental defect in the fixture.
    match husk.prove() {
        PrivateHalfOwnership::Retained(RetainReason::PossiblyCommitted) => {}
        other => {
            panic!("past P5b the ownership proof mints no token for this private half: {other:?}")
        }
    }
    assert_eq!(
        commit_record_after_error(&husk.private),
        CommitRecordPresence::Present
    );
    assert!(
        !commit_record_after_error(&husk.private).permits_deletion(),
        "the creator's half of the boundary agrees with the census's"
    );

    // And the scratch authority reclaims the same bytes.
    let root = tree.path().to_path_buf();
    assert!(husk.private.starts_with(&root) || husk.root.starts_with(&root));
    scratch_tree::remove_scratch_tree(tree.disarm())
        .expect("a tree the token minted is reclaimed whatever a fixture published in it");
    assert!(scratch_tree::proves_absent(&root), "the token root is gone");
    assert!(
        scratch_tree::proves_absent(&record),
        "and so is the record the fixture published in it"
    );
}

// =======================================================================
// The refusal that is a build failure
// =======================================================================

/// The fixture that must compile, so a refusal below is a refusal rather
/// than a broken rustc invocation.
const CONTROL: &str = r#"
        extern crate upstroke;
        use std::path::Path;
        pub fn control(public: &Path, hooks: &mut upstroke::rundir::NoHooks) {
            let _ = upstroke::rundir::classify_run_dir(public);
            let _ = upstroke::rundir::remove_public_husk(public, hooks);
        }
"#;

struct BuildRefusal {
    name: &'static str,
    source: &'static str,
    /// rustc's own error code. A fixture that only asserted "this does not
    /// compile" is green when it fails for a typo.
    codes: &'static [&'static str],
    names: &'static str,
}

fn build_refusals() -> Vec<BuildRefusal> {
    vec![
        BuildRefusal {
            name: "no-proof",
            source: r#"
        extern crate upstroke;
        pub fn delete(hooks: &mut upstroke::rundir::NoHooks) {
            let _ = upstroke::rundir::remove_private_husk(hooks);
        }
"#,
            codes: &["E0061"],
            names: "remove_private_husk",
        },
        BuildRefusal {
            name: "wrong-token",
            source: r#"
        extern crate upstroke;
        use std::path::PathBuf;
        pub fn delete(hooks: &mut upstroke::rundir::NoHooks) {
            let _ = upstroke::rundir::remove_private_husk(PathBuf::from("/tmp/x"), hooks);
        }
"#,
            codes: &["E0308"],
            names: "PrivateHalfProof",
        },
        BuildRefusal {
            name: "forged-token",
            source: r#"
        extern crate upstroke;
        use std::path::PathBuf;
        pub fn forge() -> upstroke::rundir::PrivateHalfProof {
            upstroke::rundir::PrivateHalfProof {
                target: PathBuf::new(),
                public: PathBuf::new(),
                run_id: String::new(),
            }
        }
"#,
            codes: &["E0451", "E0603", "E0063"],
            names: "PrivateHalfProof",
        },
        BuildRefusal {
            name: "cloned-token",
            source: r#"
        extern crate upstroke;
        pub fn twice(proof: upstroke::rundir::PrivateHalfProof) -> upstroke::rundir::PrivateHalfProof {
            let copy = proof.clone();
            copy
        }
"#,
            codes: &["E0599"],
            names: "PrivateHalfProof",
        },
        BuildRefusal {
            name: "defaulted-token",
            source: r#"
        extern crate upstroke;
        pub fn out_of_nothing() -> upstroke::rundir::PrivateHalfProof {
            upstroke::rundir::PrivateHalfProof::default()
        }
"#,
            codes: &["E0599"],
            names: "PrivateHalfProof",
        },
        BuildRefusal {
            name: "spent-token",
            source: r#"
        extern crate upstroke;
        pub fn twice(proof: upstroke::rundir::PrivateHalfProof, hooks: &mut upstroke::rundir::NoHooks) {
            let _ = upstroke::rundir::remove_private_husk(proof, hooks);
            let _ = upstroke::rundir::remove_private_husk(proof, hooks);
        }
"#,
            codes: &["E0382"],
            names: "proof",
        },
    ]
}

/// This crate's rlib, beside the test binary that is running.
fn this_crates_rlib(deps: &Path) -> PathBuf {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(deps).expect("the deps directory").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("libupstroke-") || !name.ends_with(".rlib") {
            continue;
        }
        let when = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .expect("mtime");
        if best.as_ref().is_none_or(|(seen, _)| when > *seen) {
            best = Some((when, entry.path()));
        }
    }
    best.expect("this crate's rlib is beside its test binary").1
}

fn compile_against_this_crate(tag: &str, source: &str) -> (bool, Vec<String>, String) {
    let dir = scratch(&format!("compile-{tag}"));
    let file = dir.join("fixture.rs");
    fs::write(&file, source).expect("fixture source");
    let deps = std::env::current_exe()
        .expect("test binary")
        .parent()
        .expect("deps directory")
        .to_path_buf();
    let rlib = this_crates_rlib(&deps);
    let out = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "lib",
            "--emit",
            "metadata",
        ])
        .arg("--extern")
        .arg(format!("upstroke={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .args(["--error-format", "json"])
        .arg("--out-dir")
        .arg(&dir)
        .arg(&file)
        .output()
        .expect("rustc runs; a missing rustc is a failure of this test, never a skip");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let mut codes = Vec::new();
    for line in stderr.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["level"] != "error" {
            continue;
        }
        if let Some(code) = value["code"]["code"].as_str() {
            codes.push(code.to_owned());
        }
    }
    (out.status.success(), codes, stderr)
}

#[test]
fn a_private_half_deletion_without_a_proof_does_not_compile_for_the_stated_reason() {
    // `resource_accounting.completeness_rule`: "a private-half deletion
    // outside the proof-token funnel fails to compile". A fixture that
    // only asserted the failure would be green for a typo, so every case
    // pins rustc's own error code and the identifier its message must
    // name — and the control proves the harness compiles anything at all.
    let (ok, codes, rendered) = compile_against_this_crate("control", CONTROL);
    assert!(
        ok && codes.is_empty(),
        "the control fixture must compile, or every refusal below is meaningless:\n{rendered}"
    );

    for case in build_refusals() {
        let (ok, codes, rendered) = compile_against_this_crate(case.name, case.source);
        assert!(!ok, "`{}` must not compile", case.name);
        assert!(
            codes.iter().any(|code| case.codes.contains(&code.as_str())),
            "`{}`: expected one of {:?}, got {codes:?}\n{rendered}",
            case.name,
            case.codes
        );
        assert!(
            rendered.contains(case.names),
            "`{}`: the message must name `{}`:\n{rendered}",
            case.name,
            case.names
        );
    }
}

// =======================================================================
// The repository key
// =======================================================================

#[test]
fn the_repo_key_is_the_construction_the_packet_states() {
    // `workspace_candidates.execution_root`: "repo_key v1 =
    // hex16(sha256('upstroke-repo-key-v1' NUL canonical common git dir
    // bytes))". The expected value is computed from that sentence here,
    // and for a fixed path it is a literal computed outside this program
    // entirely — a function may not be its own oracle.
    let dir = scratch("repokey").join("git-dir");
    fs::create_dir_all(&dir).expect("git dir");
    let canonical = fs::canonicalize(&dir).expect("canonical");
    let mut bytes = b"upstroke-repo-key-v1".to_vec();
    bytes.push(0);
    bytes.extend_from_slice(canonical.as_os_str().as_encoded_bytes());
    let expected: String = format!("{:x}", Sha256::digest(&bytes))
        .chars()
        .take(16)
        .collect();
    assert_eq!(RepoKey::v1(&canonical).as_str(), expected);
    assert_eq!(expected.len(), 16, "hex16 is sixteen hex characters");

    #[cfg(unix)]
    assert_eq!(
        RepoKey::v1(Path::new("/srv/repo/.git")).as_str(),
        "e43114efb48428eb",
        "sha256(b'upstroke-repo-key-v1\\x00/srv/repo/.git')[:16], computed elsewhere"
    );

    // Distinguishing, which is the whole job: two repositories, two keys.
    assert_ne!(
        RepoKey::v1(Path::new("/srv/a/.git")),
        RepoKey::v1(Path::new("/srv/b/.git"))
    );
}

fn git(cwd: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
}

#[test]
fn every_worktree_of_one_repository_has_one_repo_key() {
    // A run created in the main checkout and a census run from a linked
    // worktree must not call each other foreign, so the key is taken over
    // the **common** git dir. A linked worktree's own git dir is
    // `<common>/worktrees/<name>`, and this proves the derivation against
    // a real one rather than against the rule that produced it.
    let root = scratch("worktreekey");
    let main = root.join("main");
    fs::create_dir_all(&main).expect("main");
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "t@example.invalid"]);
    git(&main, &["config", "user.name", "t"]);
    fs::write(main.join("f"), "x").expect("file");
    git(&main, &["add", "f"]);
    git(&main, &["commit", "-q", "-m", "one"]);
    let linked = root.join("linked");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().expect("utf-8"),
        ],
    );

    let main_git_dir = Workspace::open(&main)
        .expect("main workspace")
        .worktree_git_dir()
        .expect("main git dir");
    let linked_git_dir = Workspace::open(&linked)
        .expect("linked workspace")
        .worktree_git_dir()
        .expect("linked git dir");
    assert_ne!(
        main_git_dir, linked_git_dir,
        "the two worktrees really do have different git dirs, so the \
             common-dir derivation is doing work"
    );
    assert!(
        linked_git_dir.parent().and_then(Path::file_name)
            == Some(std::ffi::OsStr::new("worktrees")),
        "the layout this derivation reads: {}",
        linked_git_dir.display()
    );
    assert_eq!(
        RepoKey::for_repo(&main).expect("main key"),
        RepoKey::for_repo(&linked).expect("linked key"),
        "one repository, one key"
    );
}

// =======================================================================
// The wire the marker and the records are read back off
// =======================================================================

/// Every field the packet names for each record, written by hand.
fn marker_json() -> serde_json::Value {
    serde_json::json!({
        "run_id": "01RUN",
        "repo_key": "0123456789abcdef",
        "private_dir": "/private/runs/01RUN",
        "incarnation": "01INC",
        "pid": 4242,
        "runner_policy_sha256": "sha256:aa"
    })
}

fn owner_json() -> serde_json::Value {
    serde_json::json!({
        "run_id": "01RUN",
        "repo_key": "0123456789abcdef",
        "public_dir": "/repo/.upstroke/runs/01RUN",
        "incarnation": "01INC",
        "runner": {
            "kind": "host",
            "policy": "host-v1",
            "image": null,
            "credential_volumes": null
        }
    })
}

fn commit_json() -> serde_json::Value {
    serde_json::json!({
        "run_id": "01RUN",
        "repo_key": "0123456789abcdef",
        "public_dir": "/repo/.upstroke/runs/01RUN",
        "incarnation": "01INC",
        "run_started_sha256": "sha256:bb"
    })
}

#[test]
fn each_record_carries_exactly_the_fields_the_packet_names() {
    // Mutation witnessing cannot detect a field that was never written, so
    // this is a transcription check rather than a round trip: the payloads
    // are written out of `run_creation` and `resource_accounting` by hand,
    // every named field is asserted required, and an unknown one is
    // refused because a marker is what a census decides a deletion from.
    for (what, payload, fields) in [
        (
            "marker",
            marker_json(),
            vec![
                "run_id",
                "repo_key",
                "private_dir",
                "incarnation",
                "pid",
                "runner_policy_sha256",
            ],
        ),
        (
            "owner record",
            owner_json(),
            vec!["run_id", "repo_key", "public_dir", "incarnation", "runner"],
        ),
        (
            "commit record",
            commit_json(),
            vec![
                "run_id",
                "repo_key",
                "public_dir",
                "incarnation",
                "run_started_sha256",
            ],
        ),
    ] {
        let parses = match what {
            "marker" => serde_json::from_value::<CreatingMarker>(payload.clone()).is_ok(),
            "owner record" => serde_json::from_value::<OwnerRecord>(payload.clone()).is_ok(),
            _ => serde_json::from_value::<CommitRecord>(payload.clone()).is_ok(),
        };
        assert!(parses, "{what}: the packet's own payload must parse");

        assert_eq!(
            payload.as_object().expect("object").len(),
            fields.len(),
            "{what}: the packet names {} fields",
            fields.len()
        );

        for missing in &fields {
            let mut short = payload.clone();
            short.as_object_mut().expect("object").remove(*missing);
            let refused = match what {
                "marker" => serde_json::from_value::<CreatingMarker>(short).is_err(),
                "owner record" => serde_json::from_value::<OwnerRecord>(short).is_err(),
                _ => serde_json::from_value::<CommitRecord>(short).is_err(),
            };
            assert!(refused, "{what}: `{missing}` must be required");
        }

        let mut extra = payload.clone();
        extra
            .as_object_mut()
            .expect("object")
            .insert("unknown".to_owned(), serde_json::json!(1));
        let refused = match what {
            "marker" => serde_json::from_value::<CreatingMarker>(extra).is_err(),
            "owner record" => serde_json::from_value::<OwnerRecord>(extra).is_err(),
            _ => serde_json::from_value::<CommitRecord>(extra).is_err(),
        };
        assert!(refused, "{what}: an unknown field is refused");
    }
}

#[test]
fn what_the_funnels_write_is_what_the_packet_says_they_write() {
    // The other direction: the bytes on disk, compared against the
    // independently written payloads above rather than against whatever
    // this build happens to serialize.
    let root = scratch("wire");
    let dir = root.join("half");
    fs::create_dir_all(&dir).expect("dir");
    let marker: CreatingMarker =
        serde_json::from_value(marker_json()).expect("the packet's marker");
    stage_marker(&dir, &marker, &mut NoHooks).expect("stage");
    publish_marker(&dir, &mut NoHooks).expect("publish");
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(MARKER)).expect("read")).expect("json");
    assert_eq!(written, marker_json());

    let owner: OwnerRecord = serde_json::from_value(owner_json()).expect("the packet's owner");
    stage_owner_record(&dir, &owner, &mut NoHooks).expect("stage");
    publish_owner_record(&dir, &mut NoHooks).expect("publish");
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(OWNER_RECORD)).expect("read")).expect("json");
    assert_eq!(written, owner_json());

    let record: CommitRecord =
        serde_json::from_value(commit_json()).expect("the packet's commit record");
    stage_commit_record(&dir, &record, &mut NoHooks).expect("stage");
    publish_commit_record(&dir, &mut NoHooks).expect("publish");
    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(COMMIT_RECORD)).expect("read")).expect("json");
    assert_eq!(written, commit_json());
}

#[test]
fn the_commit_records_digest_is_over_the_exact_line_bytes() {
    // `run_creation`: "run_started_sha256 = the digest of the exact
    // run_started line bytes about to be appended". Pinned against a
    // digest computed outside this program.
    assert_eq!(
        run_started_sha256(b"abc"),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "the FIPS-180-2 example digest of `abc`"
    );
    // The newline is part of the line and therefore part of the digest.
    assert_ne!(run_started_sha256(b"abc"), run_started_sha256(b"abc\n"));

    // **A real `run_started` line, spelled noncanonically**
    // (`PR5-RUNDIR-053`). Neither input above is JSON at all, so a digest
    // computed over a *reserialized* event value falls straight back to the
    // exact bytes for both and every assertion above still holds. The only
    // input that separates the two rules is a valid line whose whitespace
    // and key order are not what a serializer would emit, and the digest it
    // must have is computed outside this program:
    //
    //   python3 -c "import hashlib,sys;
    //     print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())"
    // Built by concatenation rather than as one wrapped literal: a `\` line
    // continuation eats the indentation that follows it and rustfmt then joins
    // the line, so the bytes such a literal produces are not the bytes it looks
    // like — and which exact bytes are digested is the whole of this fixture.
    let mut noncanonical: Vec<u8> = Vec::new();
    noncanonical
        .extend_from_slice(b"{\"ts\":\"2026-08-20T00:00:00Z\" ,  \"event\":\"run_started\",");
    noncanonical.extend_from_slice(b" \"data\" : {\"run_id\":\"01NONCANON\", \"schema\":3}}\n");
    let noncanonical: &[u8] = &noncanonical;
    assert_eq!(
        serde_json::from_slice::<RunStartedHeader>(&noncanonical[..noncanonical.len() - 1])
            .expect("the fixture really is a parseable run_started")
            .event,
        "run_started",
        "a fixture a reserializing digest could not be applied to would prove nothing"
    );
    assert_eq!(
        run_started_sha256(noncanonical),
        "sha256:e0d7e8c55c48fb6c62fd452e4fa95b0a2ceebd60d0375120d35dde1fcd1fb8d9",
        "the digest of these exact bytes, including the terminating newline"
    );
    // And the two rules really do differ here, so the assertion above is
    // not passing for want of a distinction: the canonical reserialization
    // of the same value has a different digest.
    let canonical = serde_json::to_vec(
        &serde_json::from_slice::<serde_json::Value>(&noncanonical[..noncanonical.len() - 1])
            .expect("valid json"),
    )
    .expect("reserialize");
    assert_ne!(
        run_started_sha256(&canonical),
        run_started_sha256(noncanonical),
        "the fixture does not separate exact-bytes from reserialized"
    );
}

/// A `.partial` is writer-owned staging residue that **no reader ingests**
/// and no ingestion consumes (`PR5-RUNDIR-060`, `PR5-RUNDIR-061`).
///
/// `transaction_fault_matrix[17].resume_action` says both halves: "a
/// `.partial` file is writer-owned staging residue: **ignored by every
/// reader** and never pruned by the coordinator", and "the file itself is
/// **persistent run-directory content (R21) in every case**". Neither had a
/// fixture. The only driver of `ingest_answer` staged, published and
/// ingested back to back, so the shape this test builds — a valid partial
/// and *no* published answer — never existed, and nothing ever read a
/// published answer twice or looked for it afterwards. A reader that fell
/// back to the partial, and a reader that consumed its input, both looked
/// exactly like a correct one.
#[test]
fn a_staged_partial_is_never_ingested_and_a_published_answer_survives_ingestion() {
    let root = scratch("answer-residue");
    let answers = root.join("answers");
    create_dir(&answers).expect("answers");

    // (a) A valid partial, and nothing published.
    stage_answer(
        &answers,
        "q-1",
        &serde_json::json!({"text": "staged"}),
        &mut NoHooks,
    )
    .expect("stage");
    let partial = answers.join("q-1.json.partial");
    let staged_bytes = fs::read(&partial).expect("the partial exists");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&staged_bytes).is_ok(),
        "the partial is valid JSON, so a fallback reader would happily return it"
    );
    assert!(
        !answers.join("q-1.json").exists(),
        "and nothing is published, which is the state the entry is about"
    );

    assert_eq!(
        ingest_answer(&answers, "q-1", &mut NoHooks).expect("ingest"),
        None,
        "a reader that fell back to the partial would answer with staging residue"
    );
    assert_eq!(
        fs::read(&partial).expect("the partial"),
        staged_bytes,
        "and the read-only ingestion left the partial byte-identical"
    );

    // (b) Published, ingested — and still there afterwards.
    publish_answer(&answers, "q-1", &mut NoHooks).expect("publish");
    let published = answers.join("q-1.json");
    let published_bytes = fs::read(&published).expect("the published answer");
    let first = ingest_answer(&answers, "q-1", &mut NoHooks).expect("ingest");
    assert!(
        first.is_some(),
        "the published answer is what a reader gets"
    );
    assert!(
        published.is_file(),
        "R21 is persistent run-directory content: ingestion is a read, not a take"
    );
    assert_eq!(
        fs::read(&published).expect("the published answer"),
        published_bytes,
        "with its original bytes"
    );
    assert_eq!(
        ingest_answer(&answers, "q-1", &mut NoHooks).expect("ingest again"),
        first,
        "so a second reader gets the same answer as the first"
    );
}

/// The moved payload writers keep the **legacy byte shape**
/// (`PR5-RUNDIR-058`).
///
/// `production_effect`: "shared primitives move behind funnels
/// behavior-neutrally". Every consumer of these three parses the JSON back,
/// so indentation and the final newline were unobserved and switching
/// `report.json` and a question payload from the moved pretty writer to a
/// compact `serde_json::to_vec` changed nothing any test could see. The
/// expected bytes are written out here rather than produced by calling the
/// writer, so this is a golden file rather than a round trip — a round trip
/// is satisfied by any serializer at all.
#[test]
fn the_payload_writers_keep_their_exact_legacy_bytes() {
    let root = scratch("golden-bytes");
    let public = root.join("public");
    let questions = public.join("questions");
    create_dir(&questions).expect("questions");
    let payload = serde_json::json!({"kind": "choice", "options": ["a", "b"]});
    let expected = "{\n  \"kind\": \"choice\",\n  \"options\": [\n    \"a\",\n    \"b\"\n  ]\n}\n";

    write_report(&public, &payload, &mut NoHooks).expect("report");
    assert_eq!(
        fs::read_to_string(public.join("report.json")).expect("report.json"),
        expected,
        "report.json is pretty-printed with two-space indentation and ends in a newline"
    );

    write_question_payload(&questions, "q-1", &payload, &mut NoHooks).expect("question");
    assert_eq!(
        fs::read_to_string(questions.join("q-1.json")).expect("q-1.json"),
        expected,
        "and a question payload is written the same way"
    );

    // The plan is a byte pass-through — it is handed bytes that are already
    // serialized and normalized — so its golden property is that nothing
    // touches them at all, trailing newline included.
    let normalized = b"{\"tasks\":[]}";
    write_plan(&public, normalized, &mut NoHooks).expect("plan");
    assert_eq!(
        fs::read(public.join(PLAN)).expect("plan.json"),
        normalized,
        "the plan's exact bytes reach disk unaltered"
    );
}

// =======================================================================
// What `status` says about a husk id
// =======================================================================

#[test]
fn a_husk_id_reports_as_one_of_the_three_things_it_can_be() {
    // `startup_census`: status "asked explicitly for a husk id, reports an
    // unstarted husk that the next write command reclaims, a retained husk
    // with its reason and locator, or a possibly committed run whose
    // public log has no valid committed first line".
    let unstarted = BoundHusk::new("statusunstarted");
    fs::create_dir_all(unstarted.public()).expect("public");
    let report = husk_report(
        &unstarted.repo,
        BOUND_RUN,
        &unstarted.repo_key,
        &unstarted.private_root,
    );
    assert!(
        matches!(report.disposition, HuskDisposition::Unstarted(_)),
        "{:?}",
        report.disposition
    );
    assert!(report.disposition.describe().contains("unstarted"));
    assert!(report.locator.is_none(), "a bare husk records no locator");

    let retained = BoundHusk::new("statusretained");
    retained.publish();
    write(&retained.private.join(OWNER_RECORD), b"{ not json");
    let report = husk_report(
        &retained.repo,
        BOUND_RUN,
        &retained.repo_key,
        &retained.private_root,
    );
    assert!(report.disposition.describe().starts_with("a retained husk"));
    assert_eq!(report.locator.as_deref(), Some(retained.private.as_path()));

    let committed = BoundHusk::new("statuspossibly");
    committed.publish();
    write(&committed.private.join(COMMIT_RECORD), b"{}");
    let report = husk_report(
        &committed.repo,
        BOUND_RUN,
        &committed.repo_key,
        &committed.private_root,
    );
    assert!(
        report.disposition.describe().contains("possibly committed"),
        "{}",
        report.disposition.describe()
    );
    assert!(
        report.disposition.describe().contains("nothing is deleted"),
        "and says so"
    );

    // The three sentences are three sentences.
    let mut said: Vec<String> = [&unstarted, &retained, &committed]
        .iter()
        .map(|husk| {
            husk_report(&husk.repo, BOUND_RUN, &husk.repo_key, &husk.private_root)
                .disposition
                .describe()
        })
        .collect();
    said.sort();
    said.dedup();
    assert_eq!(said.len(), 3, "each of the three reads differently");
}

#[test]
fn an_ambiguous_husk_prefix_is_not_reported_as_one_husk() {
    let repo = scratch("ambiguoushusk").join("repo");
    for husk in ["01HUSKA", "01HUSKB"] {
        fs::create_dir_all(runs_root(&repo).join(husk)).expect("husk");
    }
    let error = resolve_run_id(&repo, "01HUSK").expect_err("no committed run");
    assert!(
        !error.to_string().contains("never recorded a committed"),
        "two husks match, so naming one of them would be a guess: {error}"
    );
}
