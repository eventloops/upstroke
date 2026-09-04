// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/workspace_manager.rs` -- the shape
// `src/runner/container/tests.rs` and `src/agent/proc/test_support/readiness.rs`
// established for a funnel's out-of-line child. This file builds the scratch
// repositories the Worktree/Snapshot/Ref/Object suites measure against, so it
// names `fs::write`, `fs::create_dir_all` and `std::process::Command` directly.
//
// `PR6-LANEF-004`: a Rust lint level is scoped by the MODULE TREE and not by
// the file, so without an attribute here the parent's inner allow of all three
// would reach this file silently and no reviewed record would name the file
// doing the work. `clippy::disallowed_macros` is RE-DENIED rather than
// inherited -- measured at zero sites -- so a `println!` here is still a build
// error. `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::disallowed_macros)]

use super::*;

// `OsStr` came from the parent's import list until the `m4-workspace` split
// moved its last production user into a child; named here for the same reason.
use std::ffi::OsStr;
use std::sync::atomic::{AtomicU32, Ordering};

// -----------------------------------------------------------------------
// Fixtures
// -----------------------------------------------------------------------

pub(crate) static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// A scratch directory unique to this process *and* to this call, because
/// the suite runs tests in parallel and two fixtures sharing a directory
/// would each measure the other's Git repository.
pub(crate) fn scratch(tag: &str) -> PathBuf {
    let ordinal = SCRATCH.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "upstroke-wm-{tag}-{}-{ordinal}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

pub(crate) fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("run git")
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_out(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A real repository, a real private root, and a manager over both.
/// The fixture's run id: a canonical ULID, as `derive` requires
/// (`DESIGN.md` §15, "run-id = ULID"), spelt to be recognisable in a path.
pub(crate) const RUN_ID: &str = "01KZSWEEP00000000000000001";

pub(crate) struct Fixture {
    pub(crate) root: PathBuf,
    pub(crate) base: PathBuf,
    pub(crate) private: PathBuf,
    pub(crate) manager: WorkspaceManager,
    /// The first commit.
    pub(crate) seed: String,
    /// The tip of `main`.
    pub(crate) head: String,
    /// A commit on a side branch, based on `seed`, for the cherry-picks.
    pub(crate) side: String,
}

impl Fixture {
    /// A SHA-1 repository, whatever `GIT_DEFAULT_HASH` says in the
    /// environment: the object format is part of what a test asserts about
    /// (an object id's length, the null id's spelling), so the fixture pins
    /// it rather than inheriting it (§12). [`Self::with_object_format`] is
    /// the other format.
    pub(crate) fn new(tag: &str) -> Self {
        Self::with_object_format(tag, "sha1")
    }

    /// A repository of the given object format, `sha1` or `sha256`.
    pub(crate) fn with_object_format(tag: &str, object_format: &str) -> Self {
        let root = scratch(tag);
        let base = root.join("repo");
        let private = root.join("private");
        fs::create_dir_all(&base).expect("repo directory");
        fs::create_dir_all(&private).expect("private root");

        let object_format = format!("--object-format={object_format}");
        git(&base, &["init", "-q", "-b", "main", &object_format]);
        git(&base, &["config", "user.email", "tests@upstroke.local"]);
        git(&base, &["config", "user.name", "upstroke tests"]);
        // `git worktree add` writes a reflog entry; keep the repository
        // self-contained so nothing depends on a global config.
        git(&base, &["config", "core.logAllRefUpdates", "true"]);
        fs::write(base.join("a.txt"), "one\n").expect("seed file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "seed"]);
        let seed = git(&base, &["rev-parse", "HEAD"]);

        fs::write(base.join("b.txt"), "two\n").expect("second file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "second"]);
        let head = git(&base, &["rev-parse", "HEAD"]);

        git(&base, &["checkout", "-q", "-b", "side", &seed]);
        fs::write(base.join("c.txt"), "side\n").expect("side file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "side"]);
        let side = git(&base, &["rev-parse", "HEAD"]);
        git(&base, &["checkout", "-q", "main"]);

        let manager =
            WorkspaceManager::derive(&base, &private, RUN_ID, "inc-1").expect("derive the manager");
        Self {
            root,
            base,
            private,
            manager,
            seed,
            head,
            side,
        }
    }

    /// Re-open a fixture a **previous process** built.
    ///
    /// A kill child dies by `std::process::abort()`, so its `Drop` never
    /// runs and its scratch tree survives it. The parent then has to speak
    /// about that tree — which repository, which private root, which
    /// commits — and re-deriving it is the only honest way: a value passed
    /// through an environment variable would be the child's belief about
    /// its own state, and the whole point of a kill test is that the
    /// child's beliefs did not survive.
    ///
    /// The manager is derived with the **same** run id and incarnation as
    /// [`Self::new`], because an intent records both and a reclaim that
    /// derived a different pair would be reclaiming another run's residue.
    pub(crate) fn adopt(root: PathBuf) -> Self {
        let base = root.join("repo");
        let private = root.join("private");
        let head = git(&base, &["rev-parse", "main"]);
        let seed = git(&base, &["rev-parse", "main~1"]);
        let side = git(&base, &["rev-parse", "side"]);
        let manager = WorkspaceManager::derive(&base, &private, RUN_ID, "inc-1")
            .expect("derive the manager over an adopted fixture");
        Self {
            root,
            base,
            private,
            manager,
            seed,
            head,
            side,
        }
    }

    pub(crate) fn created(tag: &str) -> Self {
        let fixture = Self::new(tag);
        fixture
            .manager
            .create_execution_root(&mut NoHooks)
            .expect("create the execution root");
        fixture
    }

    /// [`Self::created`] over a SHA-256 repository, for the tests that assert
    /// something about both object formats.
    pub(crate) fn created_sha256(tag: &str) -> Self {
        let fixture = Self::with_object_format(tag, "sha256");
        fixture
            .manager
            .create_execution_root(&mut NoHooks)
            .expect("create the execution root");
        fixture
    }

    pub(crate) fn task(&self, key: &str, generation: u32) -> Slot {
        Slot::Task {
            key: key.to_owned(),
            generation,
        }
    }

    /// A task worktree at `head`, intent first.
    pub(crate) fn add_task(&self, hooks: &mut dyn EffectHooks, key: &str, generation: u32) -> Slot {
        let slot = self.task(key, generation);
        self.manager
            .write_intent(hooks, &slot)
            .expect("write the intent");
        self.manager
            .add_worktree(hooks, &slot, &self.head)
            .expect("add the worktree");
        slot
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// -----------------------------------------------------------------------
// The primitives a topology module's test cannot reach for itself
// -----------------------------------------------------------------------

/// Write `bytes` at `path`, creating the parent directories.
///
/// This is a test's *worker*: in production an agent subprocess edits files
/// and the engine never does (DESIGN.md §4). A test has no agent, so it
/// writes what the agent would have written — and it does it here, where
/// the write is inside the reviewed funnel module, rather than in the
/// topology module whose whole point is that it cannot.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the parent directory of a fixture file");
    }
    fs::write(path, bytes).expect("write a fixture file");
}

/// Create `path` and every missing parent.
pub(crate) fn create_dir(path: &Path) {
    fs::create_dir_all(path).expect("create a fixture directory");
}

/// Remove `path` if it is there. Idempotent, like every reclaim.
pub(crate) fn remove_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("removing {}: {error}", path.display()),
    }
}

/// Run this test binary again, `--exact --ignored`, with `env` set, and
/// return its exit status.
///
/// The kill-test shape `src/rundir.rs` established: `Injection::Kill` is
/// `std::process::abort()`, a real process death, so the child has to be a
/// real process and the claim is what it left on disk. `env` is a list
/// rather than a map so a caller can pass the same key twice and see the
/// last win, exactly as `Command` does.
pub(crate) fn run_kill_child(test: &str, env: &[(&str, &OsStr)]) -> std::process::ExitStatus {
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    command.status().expect("spawn the kill child")
}

/// A `git` child a test can kill at a chosen moment.
///
/// The residue sampler's child, and deliberately **blind to what it is
/// running**: no argv reaches [`Self::kill`], so a per-command count taken
/// over these cannot be defeated inside this type. It is the same shape
/// `mod tests`'s `SampledChild` uses, which stays there because the
/// four-command sampler stays there; this one exists because the
/// two-command sampler of `T-ATTEMPT` lives in a module that cannot name
/// [`Command`].
pub(crate) struct KillableGitChild {
    child: std::process::Child,
    /// Started once the spawn has returned, so what [`Self::kill`] reads
    /// off it is time the child was left *running*.
    spawned: std::time::Instant,
    /// What the clock said when a kill fired at this child, or `None` if
    /// none ever did. Written only by [`Self::kill`].
    fired: Option<std::time::Duration>,
}

impl KillableGitChild {
    /// Spawn `git -C cwd <args>` with its streams discarded.
    pub(crate) fn spawn(cwd: &Path, args: &[String]) -> Self {
        let child = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the sampled git child");
        Self {
            child,
            spawned: std::time::Instant::now(),
            fired: None,
        }
    }

    /// Kill the child, recording when the kill fired.
    ///
    /// The clock is read at the instant of the kill and stored *after* the
    /// kill returns, so deleting `self.child.kill()` leaves `outcome`
    /// unbound and the module stops compiling.
    pub(crate) fn kill(&mut self) {
        let fired = self.spawned.elapsed();
        let outcome = self.child.kill();
        self.fired = Some(fired);
        let _ = outcome;
    }

    /// Whether the child has exited on its own, and how long it took.
    ///
    /// The sampler races a measured duration, and when its measurement is
    /// wrong every kill lands after the child is already gone. Wall time
    /// from spawn to reap cannot tell it so: that clock includes the
    /// scheduled sleep, so an over-long schedule reports itself back as the
    /// duration it should have been. This reports the child's **own** time,
    /// which is the only number a recalibration can honestly use.
    ///
    /// `None` while it is still running.
    pub(crate) fn exited(&mut self) -> Option<std::time::Duration> {
        match self.child.try_wait() {
            Ok(Some(_)) => Some(self.spawned.elapsed()),
            _ => None,
        }
    }

    /// Reap it. The wait status is the only thing a kill changes.
    pub(crate) fn wait(&mut self) -> std::process::ExitStatus {
        self.child.wait().expect("reap the sampled git child")
    }

    /// When a kill fired at this child, if one ever did.
    pub(crate) fn fired(&self) -> Option<std::time::Duration> {
        self.fired
    }
}

/// Whether `status` is the death `std::process::abort()` produces.
///
/// **Not `!status.success()`.** A kill child that reaches its own
/// `unreachable!` panics, and a panic also fails to succeed — so a parent
/// that accepted any unsuccessful exit would read "the injection stopped
/// killing" as "the injection killed", and would then go on to inspect a
/// directory the panicking child's `Drop` had already deleted. Measured:
/// exactly that, on a kill armed at a site the child never reached.
///
/// Unix has a value for it — `SIGABRT`, which no Rust panic raises. Windows
/// does not expose one portably (`abort()` reaches `__fastfail`, whose code
/// has moved between CRT versions), so there the oracle is the *negation*
/// of the panic's own exit code, which `std::process::abort` cannot produce
/// and `panic!` always does.
pub(crate) fn died_by_abort(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(status) == Some(libc::SIGABRT)
    }
    #[cfg(windows)]
    {
        /// What a Rust process exits with when a panic unwinds out of main.
        const PANIC: i32 = 101;
        !status.success() && status.code() != Some(PANIC)
    }
}

/// Whether `status` carries this platform's signature of a
/// [`std::process::Child::kill`].
///
/// A **value** per platform, not `!status.success()`: a command that merely
/// failed also fails to succeed, and reading that as a kill is how a
/// kill-count keeps counting after the kill is gone.
pub(crate) fn died_by_kill(status: &std::process::ExitStatus) -> bool {
    // `Child::kill` sends `SIGKILL`, and no exit a child reaches on its own
    // carries a signal at all.
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(status) == Some(libc::SIGKILL)
    }
    // `Child::kill` is `TerminateProcess(handle, 1)`; the sampler's probe
    // asserts the same command exits 0 when nothing kills it, so 1 is not
    // an end these commands reach by themselves.
    #[cfg(windows)]
    {
        status.code() == Some(1)
    }
}

/// Time one uninterrupted run of `git -C cwd <args>`.
///
/// The kill ladder is fractions of this duration, which is the only
/// variance a replay can pin — see `mod tests`'s `measure_budget` for the
/// argument, and for why the measurement runs in a **probe slot of its
/// own** rather than in the worktree the samples will kill in.
pub(crate) fn time_git(cwd: &Path, args: &[String]) -> std::time::Duration {
    let start = std::time::Instant::now();
    let output = git_out(cwd, &args.iter().map(String::as_str).collect::<Vec<_>>());
    let elapsed = start.elapsed();
    assert!(
        output.status.success(),
        "the probe must really run: git {args:?} in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    elapsed
}
