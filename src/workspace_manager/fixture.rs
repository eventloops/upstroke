// Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
// attachment to `src/workspace_manager.rs` -- the shape
// `src/runner/container/tests.rs` and `src/agent/proc/test_support/readiness.rs`
// established for a funnel's out-of-line child. This file builds the scratch
// repositories the Worktree/Snapshot/Ref/Object suites measure against, so it
// names the `std::fs` creation, write and removal primitives, `libc::kill`, and
// `std::process::Command` directly.
//
// `PR6-LANEF-004`: a Rust lint level is scoped by the MODULE TREE and not by
// the file, so without an attribute here the parent's inner allow of all three
// would reach this file silently and no reviewed record would name the file
// doing the work. `clippy::disallowed_macros` is RE-DENIED rather than
// inherited -- measured at zero sites -- so a `println!` here is still a build
// error. `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(clippy::disallowed_macros)]

//! The scratch repository, private root and manager every `workspace_manager`
//! suite measures against, and the primitives a topology module's test cannot
//! reach for itself.
//!
//! **Everything here is test support, so §7's panic policy permits `expect` and
//! `panic!`** — and asks for what they say. Every one names the fixture step it
//! was performing and the value it was performing it on, because a fixture that
//! fails anonymously turns a defect in the code under test into a confusing
//! failure in the setup.
//!
//! # What the fixture pins, and what it deliberately inherits
//!
//! A fixture that leaves something to ambient configuration measures the
//! machine as much as the code. `PR126-REVIEW2-NULL-TESTS-INHERIT-THE-HASH-\
//! FORMAT` was one of these: `git init` reads `GIT_DEFAULT_HASH`, so a SHA-256
//! environment silently invalidated a test whose premise was an object id's
//! length. So the repository this module builds, and every Git command it runs
//! over one, is a function of its inputs:
//!
//! - **the object format**, `--object-format` on `git init` rather than the
//!   environment's default ([`ObjectFormat`]);
//! - **the initial branch**, `-b main` rather than `init.defaultBranch`;
//! - **the line endings**, `core.autocrlf=false` and `core.eol=lf` in the
//!   repository's own config, so that they bind the manager's checkouts too,
//!   and `core.attributesFile` and `GIT_ATTR_NOSYSTEM` on the fixture's own
//!   commands, because an attributes file's `text` or `eol` overrides that
//!   config and would defeat the pin;
//! - **the hooks**, `core.hooksPath` at a name nothing creates. Measured on git
//!   2.43.0: a `post-checkout` hook reached through an ambient
//!   `core.hooksPath` runs during `git worktree add` and `git checkout` and
//!   writes into the new tree, which is an effect no site accounts for and a
//!   file every later assertion sees. `init.templateDir` and `GIT_TEMPLATE_DIR`
//!   reach the same place by copying hooks into `.git/hooks` at `git init`, and
//!   the same pin refuses both;
//! - **the fsmonitor**, `core.fsmonitor=false` both in the repository's config
//!   and on each command, because the daemon an ambient setting starts holds
//!   the worktree open and every removal this suite measures then fails for a
//!   reason that is not the one under test;
//! - **the committer**, the six `GIT_AUTHOR_*`/`GIT_COMMITTER_*` variables.
//!   Environment identity overrides repository config, so the `user.name` and
//!   `user.email` written into the repository are not what decides; with the
//!   dates pinned as well, a fixture's `seed`, `head` and `side` are the same
//!   object ids on every machine;
//! - **Git's repository-locating environment**, removed rather than pinned
//!   ([`LOCAL_ENV_VARS`]). Measured on git 2.43.0: with `GIT_DIR` set,
//!   `git -C <fresh> init -b main` creates no repository at `<fresh>` at all —
//!   it re-initialises the repository `GIT_DIR` names, exits 0, and every
//!   command that follows reads and *commits into* that repository. A suite run
//!   from a Git hook, `git rebase --exec` or `git bisect run` has those
//!   variables set;
//! - **signing**, `commit.gpgsign=false`, and **the ignore file**,
//!   `core.excludesFile` at a name nothing creates: neither can change what a
//!   test observes silently, but both fail the whole suite on a machine
//!   configured for them, which §12 asks a hermetic test not to depend on.
//!
//! Three things are inherited on purpose. **The `git` binary** on `PATH`: these
//! suites are explicitly integration tests against the installed Git, and the
//! version is what several of the claims above are measured against. **The
//! system config file**, because Git for Windows writes install-time settings
//! there and `GIT_CONFIG_NOSYSTEM` would drop them all; the settings that
//! matter are pinned individually above instead. **Object replacement**:
//! `GIT_NO_REPLACE_OBJECTS` is stripped and not re-set, so the fixture reads
//! the repository the way Git reads it, because one of these suites installs a
//! replacement deliberately and asserts on it. The manager sets that variable
//! on its own commands, for its own reason, which `WorkspaceManager::command`
//! states.

use super::*;

// `OsStr` came from the parent's import list until the `m4-workspace` split
// moved its last production user into a child; named here for the same reason.
use std::cell::{Cell, RefCell};
use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

// -----------------------------------------------------------------------
// Fixtures
// -----------------------------------------------------------------------

static SCRATCH: AtomicU32 = AtomicU32::new(0);

// -----------------------------------------------------------------------
// Observing the removal retry
// -----------------------------------------------------------------------

/// What to run after each of this thread's removal attempts.
///
/// A named alias because the raw shape trips `clippy::type_complexity`, and
/// because the two `thread_local!` slots below read better for having it.
type AttemptObserver = Box<dyn FnMut(u32)>;

// Attempts `remove_tree_once_handles_close` has made **on this thread**, and
// the observer to run after each.
//
// Thread-local rather than global, and that is the whole reason it is sound:
// the suite runs tests in parallel and several of them remove worktrees, so a
// process-wide counter would be another test's number as often as this one's.
// The primitive runs on the thread that called `remove_worktree`, so a
// thread-local counts exactly the removals the observing test drove.
//
// `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN` is why the parent's half of this seam
// is declared at the bottom of `src/workspace_manager.rs` rather than beside
// the primitive: `effects::production_region` truncates a source at its first
// `#[cfg(test)]`, so a `#[cfg(test)]` item above the funnels would take every
// one of them out of the census that proves the Worktree group has them.
//
// `//` rather than `///`: rustdoc does not document a macro invocation, and
// `-D unused-doc-comments` says so.
thread_local! {
    static REMOVAL_ATTEMPTS: Cell<u32> = const { Cell::new(0) };
    static REMOVAL_ATTEMPT_OBSERVER: RefCell<Option<AttemptObserver>> =
        const { RefCell::new(None) };
}

/// A live observation of this thread's removal attempts, ended by dropping it.
///
/// Held by the observing test for exactly as long as the observation is wanted;
/// its `Drop` uninstalls the observer, so a test that unwinds cannot leave a
/// closure behind for whatever runs next on this thread.
pub(crate) struct AttemptObservation {
    /// Not `Send`: the counter and the observer are this thread's.
    _not_send: PhantomData<*const ()>,
}

impl AttemptObservation {
    /// Attempts made since the observation began.
    pub(crate) fn count(&self) -> u32 {
        REMOVAL_ATTEMPTS.with(Cell::get)
    }
}

impl Drop for AttemptObservation {
    fn drop(&mut self) {
        REMOVAL_ATTEMPT_OBSERVER.with(|slot| {
            if let Ok(mut slot) = slot.try_borrow_mut() {
                *slot = None;
            }
        });
    }
}

/// Start counting this thread's removal attempts, running `observer` after each.
///
/// The observer runs **on the removing thread, after the attempt has already
/// returned**, which is the property the closing-handle control is built on: a
/// test that releases a held handle from here knows the attempt it is releasing
/// against has completed, rather than hoping it has. Nothing outside the loop
/// can establish that, which is why this seam exists at all.
pub(crate) fn observe_removal_attempts(observer: AttemptObserver) -> AttemptObservation {
    REMOVAL_ATTEMPTS.with(|count| count.set(0));
    REMOVAL_ATTEMPT_OBSERVER.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = Some(observer);
        }
    });
    AttemptObservation {
        _not_send: PhantomData,
    }
}

/// Record that attempt `attempt` has completed, and run the observer.
///
/// Called from `super::note_removal_attempt`, the `#[cfg(test)]` half of the
/// primitive's seam. `try_borrow_mut` rather than `borrow_mut` so that an
/// observer which somehow removes a tree of its own is a no-op here instead of
/// a panic inside production code.
pub(crate) fn note_removal_attempt(attempt: u32) {
    REMOVAL_ATTEMPTS.with(|count| count.set(attempt));
    REMOVAL_ATTEMPT_OBSERVER.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if let Some(observer) = slot.as_mut() {
                observer(attempt);
            }
        }
    });
}

/// A scratch directory unique to this process *and* to this call, because
/// the suite runs tests in parallel and two fixtures sharing a directory
/// would each measure the other's Git repository.
///
/// `tag` is validated as one path component by the manager's own
/// [`safe_component`], so a tag carrying a separator cannot put the fixture's
/// tree somewhere [`Fixture::drop`] will not remove it from.
///
/// # Panics
///
/// If `tag` is not a safe component, or if the directory cannot be made —
/// including the case [`scratch_at`] exists to refuse.
pub(crate) fn scratch(tag: &str) -> PathBuf {
    if let Err(why) = safe_component(tag) {
        panic!("the fixture's scratch tag `{tag}` is not one path component: {why}");
    }
    let ordinal = SCRATCH.fetch_add(1, Ordering::SeqCst);
    scratch_at(std::env::temp_dir().join(format!(
        "upstroke-wm-{tag}-{}-{ordinal}",
        std::process::id()
    )))
}

/// Make `dir` an empty directory of this call's own, or say why it is not.
///
/// **The name is predictable and the process id in it is reusable**, so a tree
/// a previous run left behind — a kill child dies by `std::process::abort()`
/// and its `Drop` never runs, so it always leaves one — can be sitting at this
/// path. Removing it first is the whole point; what matters is that a removal
/// that *failed* is not read as one that succeeded. `create_dir_all` returns
/// `Ok` for a directory that is already there, so the two together used to hand
/// back another run's residue as a fresh fixture and nothing said so. The
/// removal's error is inspected (only `NotFound` is absence, §7) and the
/// creation is `create_dir`, which is exclusive (§8): the directory this
/// returns was made here.
fn scratch_at(dir: PathBuf) -> PathBuf {
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!(
            "a previous run left {} behind and this one could not remove it ({error}); \
             remove it and re-run, because adopting it would measure that run's residue",
            dir.display()
        ),
    }
    if let Err(error) = fs::create_dir(&dir) {
        panic!("creating the scratch directory {}: {error}", dir.display());
    }
    dir
}

/// A name nothing under a fixture directory ever creates.
///
/// `core.hooksPath`, `core.attributesFile` and `core.excludesFile` are all
/// pointed at it. Git runs no hook from a path that does not exist and ignores
/// an attributes or excludes file that is not there — the same "absence is
/// allowed" `WorkspaceManager::revalidate_hooks_path` states for the manager's
/// own hooks directory.
const ABSENT: &str = "upstroke-fixture-absent";

/// Git's repository-locating environment: `git rev-parse --local-env-vars` on
/// git 2.43.0.
///
/// Every one is removed from every command this module runs, so that a suite
/// started from inside another repository's Git — a hook, `git rebase --exec`,
/// `git bisect run` — builds its fixtures in the scratch tree and not in that
/// repository. `the_variables_git_calls_local_are_the_ones_the_fixture_strips`
/// asks the installed Git for its own list and fails if it has grown one this
/// does not carry; a name Git has since retired stays here harmlessly.
const LOCAL_ENV_VARS: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// The one place every Git command this module runs is built, and so the one
/// place its configuration and environment are decided.
///
/// The module doc lists what is pinned here and what is inherited on purpose,
/// and why each one is in the list it is in. `-c` rather than repository config
/// wherever the setting has to bind a directory this module did not build:
/// [`KillableGitChild::spawn`] and [`time_git`] take a caller's `cwd`, which is
/// usually a linked worktree and need not be a fixture's at all.
///
/// `protocol.file.allow`, which the manager pins on its own commands, is not
/// here: nothing this module runs speaks the file transport.
fn git_command<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Command {
    let absent = dir.join(ABSENT);
    let mut command = Command::new("git");
    command.arg("-C").arg(dir);
    for key in ["core.hooksPath", "core.attributesFile", "core.excludesFile"] {
        let mut setting = OsString::from(key);
        setting.push("=");
        setting.push(&absent);
        command.arg("-c").arg(setting);
    }
    command
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "commit.gpgsign=false"])
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", COMMITTER_NAME)
        .env("GIT_AUTHOR_EMAIL", COMMITTER_EMAIL)
        .env("GIT_AUTHOR_DATE", COMMITTER_DATE)
        .env("GIT_COMMITTER_NAME", COMMITTER_NAME)
        .env("GIT_COMMITTER_EMAIL", COMMITTER_EMAIL)
        .env("GIT_COMMITTER_DATE", COMMITTER_DATE)
        .args(args)
        .stdin(Stdio::null());
    for name in LOCAL_ENV_VARS {
        command.env_remove(name);
    }
    command
}

/// The identity every fixture commit is made under. Written into the
/// repository's config as well, so a command run by hand in one of these
/// repositories while debugging has an identity too.
const COMMITTER_NAME: &str = "upstroke tests";
/// The address beside [`COMMITTER_NAME`].
const COMMITTER_EMAIL: &str = "tests@upstroke.local";
/// The epoch, so that a fixture's commits are the same object ids on every
/// machine and in every run.
const COMMITTER_DATE: &str = "@0 +0000";

/// Run `git` in `dir` and hand back what it did, whatever its exit status.
///
/// # Panics
///
/// If the child could not be started at all, naming the command.
pub(crate) fn git_out(dir: &Path, args: &[&str]) -> Output {
    git_command(dir, args).output().unwrap_or_else(|error| {
        panic!(
            "the fixture could not run `git {args:?}` in {}: {error}",
            dir.display()
        )
    })
}

/// Run `git` in `dir`, require it to succeed, and return its trimmed stdout.
///
/// # Panics
///
/// If the command fails, quoting both streams because Git reports on stdout as
/// often as on stderr (`git commit` with nothing staged says so on stdout and
/// exits 1). If its answer is not UTF-8: this value is an identity — a commit,
/// a ref name, a path — and §8 allows a lossy string for diagnostics only, so a
/// byte Git meant is refused rather than replaced by `U+FFFD`.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_out(dir, args);
    assert!(
        output.status.success(),
        "the fixture's `git {args:?}` in {} exited {}: {}{}",
        dir.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match String::from_utf8(output.stdout) {
        Ok(text) => text.trim().to_owned(),
        Err(error) => panic!(
            "the fixture's `git {args:?}` in {} answered bytes that stop being UTF-8 at index {}, \
             and this answer is an identity rather than a diagnostic",
            dir.display(),
            error.utf8_error().valid_up_to()
        ),
    }
}

/// A real repository, a real private root, and a manager over both.
/// The fixture's run id: a canonical ULID, as `derive` requires
/// (`DESIGN.md` §15, "run-id = ULID"), spelt to be recognisable in a path.
pub(crate) const RUN_ID: &str = "01KZSWEEP00000000000000001";

/// The object format a fixture repository is built with.
///
/// An enum rather than the string `git init --object-format` takes (§5): the
/// two formats differ in what a test may assert about an object id, and
/// [`Self::hex_len`] is that difference named once instead of at each site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectFormat {
    /// 40 hexadecimal characters.
    Sha1,
    /// 64 hexadecimal characters.
    Sha256,
}

impl ObjectFormat {
    /// The word `git init --object-format` takes.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }

    /// How many hexadecimal characters an object id of this format has.
    pub(crate) const fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

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
        Self::with_object_format(tag, ObjectFormat::Sha1)
    }

    /// A repository of the given object format.
    pub(crate) fn with_object_format(tag: &str, object_format: ObjectFormat) -> Self {
        let root = scratch(tag);
        let base = root.join("repo");
        let private = root.join("private");
        fs::create_dir_all(&base).expect("the fixture's repository directory");
        fs::create_dir_all(&private).expect("the fixture's private root");

        let format_arg = format!("--object-format={}", object_format.as_str());
        git(&base, &["init", "-q", "-b", "main", &format_arg]);
        // The identity every command carries comes from the environment, which
        // overrides this; these are here so a command run by hand in one of
        // these repositories has an identity too.
        git(&base, &["config", "user.email", COMMITTER_EMAIL]);
        git(&base, &["config", "user.name", COMMITTER_NAME]);
        // Line endings are pinned for the same reason the object format is
        // (§12, and `PR126-REVIEW2-NULL-TESTS-INHERIT-THE-HASH-FORMAT`): an
        // ambient Git setting that silently changes what a test observes.
        // With `core.autocrlf` on, as it is on the Windows guest, a blob
        // written as `A\n` is checked out as `A\r\n`, so a test comparing
        // checked-out content against what it wrote fails on that platform
        // alone while the blob is the one it asked for. In the repository's
        // own config rather than on the fixture's commands, because these two
        // have to bind the manager's checkouts as well.
        git(&base, &["config", "core.autocrlf", "false"]);
        git(&base, &["config", "core.eol", "lf"]);
        // Here for the same reason, and it reaches further than the `-c` on
        // this module's commands: the parent's `read_only_git` runs `status`
        // and `worktree list` in these repositories and sets no `-c` at all,
        // and an ambient fsmonitor would start a daemon holding the worktree
        // open under it.
        git(&base, &["config", "core.fsmonitor", "false"]);
        // `git worktree add` writes a reflog entry; keep the repository
        // self-contained so nothing depends on a global config.
        git(&base, &["config", "core.logAllRefUpdates", "true"]);
        fs::write(base.join("a.txt"), "one\n").expect("the fixture's seed file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "seed"]);
        let seed = git(&base, &["rev-parse", "HEAD"]);

        fs::write(base.join("b.txt"), "two\n").expect("the fixture's second file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "second"]);
        let head = git(&base, &["rev-parse", "HEAD"]);

        git(&base, &["checkout", "-q", "-b", "side", &seed]);
        fs::write(base.join("c.txt"), "side\n").expect("the fixture's side file");
        git(&base, &["add", "-A"]);
        git(&base, &["commit", "-q", "-m", "side"]);
        let side = git(&base, &["rev-parse", "HEAD"]);
        git(&base, &["checkout", "-q", "main"]);

        let manager = WorkspaceManager::derive(&base, &private, RUN_ID, "inc-1")
            .expect("derive the manager over the fixture's repository and private root");
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
    ///
    /// `seed` is the repository's root commit and not `main~1`. The field is
    /// documented as the first commit, and a child that committed on `main`
    /// before it died — which is what several of these tests have it do —
    /// moves `main~1` off it while leaving the root commit where it was.
    pub(crate) fn adopt(root: PathBuf) -> Self {
        let base = root.join("repo");
        let private = root.join("private");
        let head = git(&base, &["rev-parse", "main"]);
        let seed = git(&base, &["rev-list", "--max-parents=0", "main"]);
        assert!(
            !seed.contains('\n'),
            "the adopted repository at {} has more than one root commit on `main`: {seed}",
            base.display()
        );
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
        Self::new(tag).with_execution_root()
    }

    /// [`Self::created`] over a SHA-256 repository, for the tests that assert
    /// something about both object formats.
    pub(crate) fn created_sha256(tag: &str) -> Self {
        Self::with_object_format(tag, ObjectFormat::Sha256).with_execution_root()
    }

    /// The one step [`Self::created`] and [`Self::created_sha256`] differ by
    /// nothing in.
    fn with_execution_root(self) -> Self {
        self.manager
            .create_execution_root(&mut NoHooks)
            .expect("create the fixture's execution root");
        self
    }

    /// A task slot, refused here if the manager would refuse it.
    ///
    /// # Panics
    ///
    /// If `key` is not a [`safe_component`]. A slot the manager's own
    /// `Slot::validate` rejects builds no worktree and writes no intent, so a
    /// test handed one measures a refusal it did not mean to ask for; this says
    /// so at the fixture step that produced it.
    pub(crate) fn task(&self, key: &str, generation: u32) -> Slot {
        let slot = Slot::Task {
            key: key.to_owned(),
            generation,
        };
        if let Err(why) = slot.validate() {
            panic!("the fixture was asked for a task slot the manager refuses: {why}");
        }
        slot
    }

    /// A task worktree at `head`, intent first.
    pub(crate) fn add_task(&self, hooks: &mut dyn EffectHooks, key: &str, generation: u32) -> Slot {
        let slot = self.task(key, generation);
        self.manager
            .write_intent(hooks, &slot)
            .unwrap_or_else(|error| panic!("write the fixture's intent for `{key}`: {error}"));
        self.manager
            .add_worktree(hooks, &slot, &self.head)
            .unwrap_or_else(|error| panic!("add the fixture's worktree for `{key}`: {error}"));
        slot
    }
}

impl Drop for Fixture {
    /// Best effort, and the observability §7 asks of a best-effort operation is
    /// [`scratch_at`]: a tree this fails to remove is one the next run to
    /// collide with the path reports rather than adopts.
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
        fs::create_dir_all(parent).unwrap_or_else(|error| {
            panic!(
                "creating {} for the fixture file {}: {error}",
                parent.display(),
                path.display()
            )
        });
    }
    fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("writing the fixture file {}: {error}", path.display()));
}

/// Create `path` and every missing parent.
pub(crate) fn create_dir(path: &Path) {
    fs::create_dir_all(path).unwrap_or_else(|error| {
        panic!("creating the fixture directory {}: {error}", path.display())
    });
}

/// Remove `path` if it is there. Idempotent, like every reclaim.
pub(crate) fn remove_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("removing the fixture file {}: {error}", path.display()),
    }
}

/// What a libtest harness prints before it runs the one test a filter selected.
///
/// Line-buffered, so it reaches the parent even from a child that dies by
/// `std::process::abort()` a moment later (measured).
const SELECTED_ONE: &str = "running 1 test";

/// What a Rust process exits with when a panic unwinds out of main.
const PANIC_EXIT: i32 = 101;

/// Run this test binary again, `--exact --ignored`, with `env` set, and
/// return its exit status.
///
/// The kill-test shape `src/rundir.rs` established: `Injection::Kill` is
/// `std::process::abort()`, a real process death, so the child has to be a
/// real process and the claim is what it left on disk. It is not only a kill
/// shape — anything that needs a fresh process, an environment this one cannot
/// safely set for itself, runs this way. `env` is a list rather than a map so a
/// caller can pass the same key twice and see the last win, exactly as
/// [`Command`] does.
///
/// # Panics
///
/// **If the harness did not select exactly one test.** `--exact` against a name
/// no test has runs nothing and exits **0** (measured), which every caller here
/// reads as the child having completed — so a renamed or un-`#[ignore]`d child
/// reports as whatever the caller expected a successful exit to mean, and the
/// caller's own message then blames the injection. That is the one failure this
/// helper can tell apart, so it does, and it quotes what the child said.
///
/// **If the child panicked.** Its streams are captured, so a panic in the
/// child's *setup* would otherwise reach the caller as a bare exit code with
/// the diagnostic thrown away; no caller expects a panicking child, and
/// [`died_by_abort`] excludes this code on both platforms.
pub(crate) fn run_child_test(test: &str, env: &[(&str, &OsStr)]) -> std::process::ExitStatus {
    let mut command = Command::new(std::env::current_exe().expect("this test binary"));
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("spawning the child that runs `{test}`: {error}"));
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        said.contains(SELECTED_ONE),
        "the child was asked for `{test}` with `--exact --ignored` and its harness never said \
         `{SELECTED_ONE}`, so no test ran: the name is wrong, or it is not `#[ignore]`d. It said: \
         {said}"
    );
    assert!(
        output.status.code() != Some(PANIC_EXIT),
        "the child running `{test}` panicked rather than reaching its injection. It said: {said}"
    );
    output.status
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
    /// What the clock said when a kill was fired **at** this child, or `None`
    /// if none ever was. Written only by [`Self::kill`], and it counts
    /// attempts: whether a kill landed is the wait status's answer, which is
    /// what [`died_by_kill`] reads.
    fired: Option<std::time::Duration>,
}

impl KillableGitChild {
    /// Spawn `git -C cwd <args>` through [`git_command`], with its streams
    /// discarded. `cwd` is usually a linked worktree and need not be a
    /// fixture's, which is why the pins are on the command rather than in a
    /// repository's config.
    pub(crate) fn spawn(cwd: &Path, args: &[String]) -> Self {
        let child = git_command(cwd, args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "spawning the sampled git child `{args:?}` in {}: {error}",
                    cwd.display()
                )
            });
        Self {
            child,
            spawned: std::time::Instant::now(),
            fired: None,
        }
    }

    /// The child's process id, for a test that has to ask the operating system
    /// whether it is still there.
    ///
    /// `#[cfg(unix)]` because its only caller is, and an accessor nothing calls
    /// is `dead_code`, which the Windows Clippy leg refuses under `-D warnings`
    /// (measured on this pull request: `lint (windows)` at `a1d3e6a`).
    #[cfg(unix)]
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
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
    ///
    /// # Panics
    ///
    /// If the poll itself fails. A failed `try_wait` is not "still running":
    /// folding it into `None` would leave the sampler waiting for a child it
    /// can no longer ask about, and §7 does not let a failure become an
    /// absence.
    pub(crate) fn exited(&mut self) -> Option<std::time::Duration> {
        match self.child.try_wait() {
            Ok(Some(_)) => Some(self.spawned.elapsed()),
            Ok(None) => None,
            Err(error) => panic!("polling the sampled git child: {error}"),
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

impl Drop for KillableGitChild {
    /// **`std::process::Child` neither kills nor reaps on its own drop**, so a
    /// sampler that panics between [`Self::spawn`] and [`Self::wait`] — which
    /// is how any assertion in it reports — leaves a real `git` running in a
    /// worktree the harness is about to remove. §6 asks RAII to clean up on
    /// panic unwinding as well as on the ordinary path, so it happens here.
    ///
    /// Both results are dropped deliberately: after an explicit [`Self::wait`]
    /// this is the ordinary path and `kill` reports `InvalidInput`, and a
    /// failure leaves the process exactly where it would have been without
    /// this.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        !status.success() && status.code() != Some(PANIC_EXIT)
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

// -----------------------------------------------------------------------
// The fixture's own tests
// -----------------------------------------------------------------------

/// What this module promises the suites that rest on it.
///
/// A weakness here weakens every `workspace_manager` test at once, so the
/// promises are executed rather than asserted in prose: the scratch directory
/// is this call's own, the repository is a function of its inputs and not of
/// the machine, a slot the manager would refuse is refused here, and a child
/// process that ran nothing is not a child process that succeeded.
#[cfg(test)]
mod tests {
    use super::*;

    /// The harness name of [`ambient_environment_child`], which
    /// [`the_fixture_is_immune_to_the_ambient_git_environment`] runs as a
    /// child. `run_child_test` refuses a name no test has, so a rename here
    /// fails loudly rather than passing vacuously.
    const AMBIENT_CHILD: &str = "workspace_manager::fixture::tests::ambient_environment_child";

    /// The harness name of [`panicking_child`].
    const PANICKING_CHILD: &str = "workspace_manager::fixture::tests::panicking_child";

    /// The harness name of [`ambient_config_child`], run the same way.
    const CONFIG_CHILD: &str = "workspace_manager::fixture::tests::ambient_config_child";

    /// What the parent hands each child: the three commit ids its own fixture
    /// produced, which the child's must equal.
    const AMBIENT_EXPECT: &str = "UPSTROKE_TEST_AMBIENT_EXPECT";

    #[test]
    fn a_scratch_tag_that_is_not_one_path_component_is_refused() {
        let refused = std::panic::catch_unwind(|| scratch("has/separator"));
        let error = refused.expect_err("a tag with a separator must be refused");
        let message = error
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains("is not one path component"),
            "the refusal must name the tag and the objection: {message}"
        );
    }

    #[test]
    fn scratch_empties_a_directory_a_previous_run_left_at_the_same_name() {
        let dir = scratch("stale-tree");
        write_file(
            &dir.join("residue").join("left-behind.txt"),
            b"a previous run\n",
        );
        let again = scratch_at(dir.clone());
        assert_eq!(again, dir, "the same name is returned");
        let mut entries = fs::read_dir(&dir).expect("read the fresh scratch directory");
        assert!(
            entries.next().is_none(),
            "the tree the previous run left is gone rather than adopted: {}",
            dir.display()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The removal that fails is the one that used to be silent: `create_dir_all`
    /// answers `Ok` for a directory that is already there, so the fixture went on
    /// to measure another run's residue.
    ///
    /// Unix only, and it must not run as root: `remove_dir_all` needs write
    /// permission on the *parent* to unlink the entry, and under a user for whom
    /// mode bits do not bind the removal succeeds and the case is never reached.
    /// The test says so rather than passing vacuously.
    #[cfg(unix)]
    #[test]
    fn a_stale_tree_that_cannot_be_removed_is_reported_and_not_adopted() {
        use std::os::unix::fs::PermissionsExt as _;

        let holder = scratch("stale-held");
        let stale = holder.join("victim");
        write_file(&stale.join("residue.txt"), b"a previous run\n");
        fs::set_permissions(&holder, fs::Permissions::from_mode(0o555))
            .expect("make the holding directory unwritable");

        let outcome = std::panic::catch_unwind(|| scratch_at(stale.clone()));
        fs::set_permissions(&holder, fs::Permissions::from_mode(0o755))
            .expect("restore the holding directory");
        let error = outcome.expect_err(
            "a stale tree that cannot be removed must be reported; if this run could remove it \
             anyway, it is running as root and this test proves nothing",
        );
        let message = error
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains("could not remove it"),
            "the refusal must say a previous run's tree is in the way: {message}"
        );
        let _ = fs::remove_dir_all(&holder);
    }

    /// `init.templateDir` and `GIT_TEMPLATE_DIR` put hooks in `.git/hooks` at
    /// `git init`, and an ambient `core.hooksPath` points every command at a
    /// directory of them. Both run under the fixture's own `checkout` and
    /// `commit` unless `core.hooksPath` says otherwise, and what they write
    /// lands in the tree every later assertion reads.
    ///
    /// The hook writes a name relative to its own working directory, which is
    /// the worktree root, so the witness needs no path quoting and holds on
    /// every platform Git runs hooks on.
    #[test]
    fn a_hook_in_the_repositorys_own_hooks_directory_never_runs() {
        let fixture = Fixture::new("hook-planted");
        let hook = fixture
            .base
            .join(".git")
            .join("hooks")
            .join("post-checkout");
        write_file(&hook, b"#!/bin/sh\n: > hook-ran.marker\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))
                .expect("make the planted hook executable");
        }

        git(&fixture.base, &["checkout", "-q", "side"]);
        git(&fixture.base, &["checkout", "-q", "main"]);

        assert!(
            !fixture.base.join("hook-ran.marker").exists(),
            "the fixture's own checkouts ran a hook from {}",
            hook.display()
        );
    }

    /// The whole ambient-environment claim, measured in a child process because
    /// this one cannot safely set its own environment while other tests run.
    ///
    /// The child builds a fixture with `GIT_DIR`, `GIT_WORK_TREE`,
    /// `GIT_INDEX_FILE` and the six identity variables all pointed at another
    /// repository, and its three commit ids must equal the ones this process
    /// got with none of them set. The victim repository must be untouched: with
    /// `GIT_DIR` inherited, `git -C <fresh> init` re-initialises *it* and every
    /// command after that reads and commits into it (measured, git 2.43.0).
    #[test]
    fn the_fixture_is_immune_to_the_ambient_git_environment() {
        let clean = Fixture::new("ambient-clean");
        let expected = format!("{} {} {}", clean.seed, clean.head, clean.side);

        let victim = Fixture::new("ambient-victim");
        let victim_main = git(&victim.base, &["rev-parse", "main"]);
        let victim_commits = git(&victim.base, &["rev-list", "--count", "main"]);
        let victim_git_dir = victim.base.join(".git");
        let victim_index = victim_git_dir.join("index");

        let status = run_child_test(
            AMBIENT_CHILD,
            &[
                (AMBIENT_EXPECT, OsStr::new(expected.as_str())),
                ("GIT_DIR", victim_git_dir.as_os_str()),
                ("GIT_WORK_TREE", victim.base.as_os_str()),
                ("GIT_INDEX_FILE", victim_index.as_os_str()),
                ("GIT_AUTHOR_NAME", OsStr::new("ambient author")),
                ("GIT_AUTHOR_EMAIL", OsStr::new("ambient@example.invalid")),
                ("GIT_AUTHOR_DATE", OsStr::new("@1234567890 +0000")),
                ("GIT_COMMITTER_NAME", OsStr::new("ambient committer")),
                ("GIT_COMMITTER_EMAIL", OsStr::new("ambient@example.invalid")),
                ("GIT_COMMITTER_DATE", OsStr::new("@1234567890 +0000")),
            ],
        );
        assert!(
            status.success(),
            "the child built its fixture under a hostile Git environment and ended {status:?}"
        );
        assert_eq!(
            git(&victim.base, &["rev-parse", "main"]),
            victim_main,
            "the child moved `main` in the repository `GIT_DIR` named"
        );
        assert_eq!(
            git(&victim.base, &["rev-list", "--count", "main"]),
            victim_commits,
            "the child committed into the repository `GIT_DIR` named"
        );
    }

    /// The child half of [`the_fixture_is_immune_to_the_ambient_git_environment`].
    #[test]
    #[ignore = "spawned by `the_fixture_is_immune_to_the_ambient_git_environment` with a \
                hostile Git environment"]
    fn ambient_environment_child() {
        let expected =
            std::env::var(AMBIENT_EXPECT).expect("the parent's expected commit ids, in the child");
        let fixture = Fixture::new("ambient-child");
        assert_eq!(
            format!("{} {} {}", fixture.seed, fixture.head, fixture.side),
            expected,
            "the fixture's commits are not a function of its inputs alone"
        );
    }

    /// `seed` is documented as the first commit. A child that committed on
    /// `main` before it died moves `main~1` off the root commit while leaving
    /// the root commit where it was, so the parent that adopts its tree would
    /// have spoken about a different commit than the one that built it.
    #[test]
    fn adopt_re_derives_the_first_commit_after_main_has_moved_on() {
        let fixture = Fixture::new("adopt-root");
        write_file(&fixture.base.join("d.txt"), b"third\n");
        git(&fixture.base, &["add", "-A"]);
        git(&fixture.base, &["commit", "-q", "-m", "third"]);

        let adopted = Fixture::adopt(fixture.root.clone());
        assert_eq!(adopted.seed, fixture.seed, "the first commit is re-derived");
        assert_eq!(adopted.side, fixture.side, "and so is the side commit");
        assert_ne!(
            adopted.head, fixture.head,
            "`main` moved, so the adopted head is its new tip"
        );
    }

    #[test]
    fn a_task_key_the_manager_would_refuse_fails_at_the_fixture_step() {
        let fixture = Fixture::new("task-refused");
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fixture.task("has/separator", 1)
        }));
        let error = refused.expect_err("a key with a separator must be refused");
        let message = error
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains("a task slot the manager refuses"),
            "the refusal must name the fixture step: {message}"
        );
    }

    /// A commit, a ref name or a path is an identity, and §8 allows a lossy
    /// string for diagnostics only. Before this, a byte Git meant came back as
    /// `U+FFFD` and whatever compared it saw two different values as equal.
    #[test]
    #[should_panic(expected = "stop being UTF-8")]
    fn git_refuses_an_answer_that_is_not_utf8() {
        let fixture = Fixture::new("not-utf8");
        write_file(&fixture.base.join("bytes.bin"), &[0xff, 0xfe]);
        git(&fixture.base, &["add", "-A"]);
        git(&fixture.base, &["commit", "-q", "-m", "bytes"]);
        let _ = git(&fixture.base, &["cat-file", "blob", "HEAD:bytes.bin"]);
    }

    /// `--exact` against a name no test has runs nothing and exits 0, which
    /// every caller reads as the child having completed its injection.
    #[test]
    #[should_panic(expected = "running 1 test")]
    fn a_child_whose_harness_selected_no_test_is_not_a_child_that_succeeded() {
        let _ = run_child_test(
            "workspace_manager::fixture::tests::no_test_has_this_name",
            &[],
        );
    }

    /// A child that panicked before it reached its injection is not a child
    /// whose injection stopped killing, and its streams are captured, so the
    /// diagnostic would otherwise be thrown away and only an exit code reach
    /// the caller.
    #[test]
    #[should_panic(expected = "panicked rather than reaching its injection")]
    fn a_child_that_panicked_says_so_and_quotes_what_it_said() {
        let _ = run_child_test(PANICKING_CHILD, &[]);
    }

    /// The child half of [`a_child_that_panicked_says_so_and_quotes_what_it_said`].
    #[test]
    #[ignore = "spawned by `a_child_that_panicked_says_so_and_quotes_what_it_said`"]
    fn panicking_child() {
        panic!("the child's own diagnostic");
    }

    /// `std::process::Child` neither kills nor reaps on its own drop, so a
    /// sampler that panics leaves a real `git` running in a worktree the
    /// harness is about to remove.
    ///
    /// Unix only: the witness asks the operating system whether the process is
    /// still there, and `kill(pid, 0)` is that question. The mechanism is not
    /// platform-specific — `Child::drop` does nothing on either platform — but
    /// the measurement is, and this claim is made only where it was run.
    #[cfg(unix)]
    #[test]
    fn a_killable_git_child_is_killed_and_reaped_when_it_is_dropped() {
        /// Whether the operating system still knows this process.
        fn alive(pid: u32) -> bool {
            let pid = libc::pid_t::try_from(pid).expect("a process id fits in `pid_t`");
            // SAFETY: signal 0 sends nothing and only asks whether the process
            // exists and may be signalled. No pointer, length or lifetime is
            // involved, and the call cannot alias anything.
            let answer = unsafe { libc::kill(pid, 0) };
            answer == 0
        }

        let fixture = Fixture::new("child-drop");
        git(
            &fixture.base,
            &["config", "alias.upstroke-fixture-slow", "!sleep 5"],
        );

        let pid = {
            let child =
                KillableGitChild::spawn(&fixture.base, &["upstroke-fixture-slow".to_owned()]);
            let pid = child.id();
            assert!(alive(pid), "the spawned child is running");
            pid
        };
        assert!(
            !alive(pid),
            "the dropped child is still running as pid {pid}"
        );
    }

    #[test]
    fn the_object_format_is_the_one_the_fixture_asked_for() {
        for format in [ObjectFormat::Sha1, ObjectFormat::Sha256] {
            let fixture = Fixture::with_object_format("object-format", format);
            assert_eq!(
                fixture.head.len(),
                format.hex_len(),
                "{format:?} was asked for and {} came back",
                fixture.head
            );
        }
    }

    /// The list is a constant so that no command pays for a `git` invocation,
    /// and this is what keeps the constant from ageing: a variable the
    /// installed Git calls local and this module does not strip is one an
    /// ambient environment can still reach the fixture through. A name Git has
    /// since retired stays in the constant harmlessly, so the check is
    /// containment and not equality.
    #[test]
    fn the_variables_git_calls_local_are_the_ones_the_fixture_strips() {
        let dir = scratch("local-env-vars");
        let listed = git(&dir, &["rev-parse", "--local-env-vars"]);
        let named: Vec<&str> = listed
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        assert!(
            named.len() > 5,
            "the installed Git named too few local variables to be its real list: {named:?}"
        );
        let missing: Vec<&&str> = named
            .iter()
            .filter(|name| !LOCAL_ENV_VARS.contains(*name))
            .collect();
        assert!(
            missing.is_empty(),
            "the installed Git calls these local and this module does not strip them: {missing:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The other half of the ambient claim: a **configuration file** this
    /// process cannot replace for itself.
    ///
    /// The child is given a global and a system config file pinning everything
    /// the fixture pins — signing on, an ignore file that excludes the seed
    /// files, an attributes file that rewrites their line endings on checkout,
    /// a hooks directory holding a `post-checkout`, an fsmonitor, `autocrlf`
    /// and another default branch — and must still produce this process's three
    /// commit ids, this process's file content, and no hook.
    #[test]
    fn the_fixture_is_immune_to_an_ambient_global_git_config() {
        let clean = Fixture::new("config-clean");
        let expected = format!("{} {} {}", clean.seed, clean.head, clean.side);

        let hostile = clean.root.join("hostile");
        let excludes = hostile.join("ignore");
        let attributes = hostile.join("attributes");
        let hooks = hostile.join("hooks");
        let hook = hooks.join("post-checkout");
        write_file(&excludes, b"*.txt\n");
        write_file(&attributes, b"* text eol=crlf\n");
        write_file(&hook, b"#!/bin/sh\n: > hook-ran.marker\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))
                .expect("make the hostile hook executable");
        }
        let config = hostile.join("config");
        write_file(
            &config,
            format!(
                "[commit]\n\tgpgsign = true\n\
                 [init]\n\tdefaultBranch = trunk\n\
                 [core]\n\tautocrlf = true\n\
                 \tfsmonitor = {hook}\n\
                 \texcludesFile = {excludes}\n\
                 \tattributesFile = {attributes}\n\
                 \thooksPath = {hooks}\n",
                hook = escaped(&hook),
                excludes = escaped(&excludes),
                attributes = escaped(&attributes),
                hooks = escaped(&hooks),
            )
            .as_bytes(),
        );

        let status = run_child_test(
            CONFIG_CHILD,
            &[
                (AMBIENT_EXPECT, OsStr::new(expected.as_str())),
                ("GIT_CONFIG_GLOBAL", config.as_os_str()),
                ("GIT_CONFIG_SYSTEM", config.as_os_str()),
            ],
        );
        assert!(
            status.success(),
            "the child built its fixture under a hostile Git configuration and ended {status:?}"
        );
    }

    /// A path as a Git config value: the parser reads `\\` as an escape, so a
    /// Windows path written verbatim is a bad config value.
    fn escaped(path: &Path) -> String {
        path.display().to_string().replace('\\', "\\\\")
    }

    /// The child half of [`the_fixture_is_immune_to_an_ambient_global_git_config`].
    #[test]
    #[ignore = "spawned by `the_fixture_is_immune_to_an_ambient_global_git_config` with a \
                hostile Git configuration"]
    fn ambient_config_child() {
        let expected =
            std::env::var(AMBIENT_EXPECT).expect("the parent's expected commit ids, in the child");
        let fixture = Fixture::new("config-child");
        assert_eq!(
            format!("{} {} {}", fixture.seed, fixture.head, fixture.side),
            expected,
            "the fixture's commits are not a function of its inputs alone"
        );
        // `b.txt` and not `a.txt`: the fixture's last step checks `main` out
        // over `side`, which creates `b.txt` and leaves `a.txt` alone because
        // its blob did not change. Only a file a checkout actually writes goes
        // through the attribute filter, so asserting on `a.txt` alone passes
        // whether the pin holds or not (measured, with the pin deleted).
        assert_eq!(
            fs::read(fixture.base.join("b.txt")).expect("read the checked-out file back"),
            b"two\n",
            "an ambient attributes file rewrote what the checkout wrote"
        );
        assert_eq!(
            fs::read(fixture.base.join("a.txt")).expect("read the seed file back"),
            b"one\n",
            "an ambient attributes file rewrote the seed file"
        );
        assert!(
            !fixture.base.join("hook-ran.marker").exists(),
            "an ambient hooks path ran a hook during the fixture's own checkouts"
        );
    }
}
