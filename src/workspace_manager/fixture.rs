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
//! # The environment every Git command here runs in
//!
//! A fixture that leaves something to ambient configuration measures the
//! machine as much as the code, and the supply of ambient settings is
//! unbounded: `GIT_DEFAULT_HASH` invalidated an object-id test
//! (`PR126-REVIEW2-NULL-TESTS-INHERIT-THE-HASH-FORMAT`), and pinning settings
//! one at a time only moves the next one along — a template `config` carrying
//! `core.worktree`, an `i18n.commitEncoding` that adds a header to every
//! commit, an attributes file that rewrites what a checkout writes. So this
//! module does not enumerate them. **[`git_command`] clears the environment and
//! rebuilds it**, and every Git command it builds therefore sees:
//!
//! - **an allowlisted environment.** [`INHERITED`] is the whole of what a Git
//!   child inherits, each name with its reason; everything else, `GIT_*`
//!   included, is gone because the environment was cleared and not filtered.
//! - **no configuration file.** `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM`
//!   name a path that does not exist, and `GIT_CONFIG_NOSYSTEM` is set, so
//!   Git reads the repository's own config and nothing else. `HOME` and
//!   `XDG_CONFIG_HOME` name that same absent path, so there is no `~` to find
//!   one under either.
//! - **no template.** `GIT_TEMPLATE_DIR` is empty, which beats
//!   `init.templateDir` and the built-in default (measured, git 2.43.0: a
//!   `git init` under it copies nothing at all, and a template `config`
//!   setting `core.worktree` to another repository — which resolves the new
//!   repository's worktree to that one — does not arrive).
//! - **no system attributes.** `GIT_ATTR_NOSYSTEM` is set and
//!   `core.attributesFile` names the absent path, so an `eol` or `text`
//!   attribute cannot override the repository's own line-ending settings.
//! - **a fixed identity and clock.** The six `GIT_AUTHOR_*`/`GIT_COMMITTER_*`
//!   variables, because environment identity overrides repository config, so
//!   the `user.name` and `user.email` written into the repository are not what
//!   decides; and `LC_ALL` and `TZ`, so message language and timestamp
//!   rendering are not the machine's.
//! - **the settings this module states**, as `-c` on the command line: the
//!   hooks path, the fsmonitor, the attributes and excludes files, and
//!   signing, each at [`ABSENT`] or `false`.
//!
//! **So the claim is a construction rather than a list**: a Git command here
//! runs under Git's own built-in defaults, plus the repository's config, plus
//! the `-c` settings above. What is deliberately let through is exactly
//! [`INHERITED`], and the reason for each name is on the constant.
//!
//! Two boundaries, because neither is closed here. **The `git` binary** is
//! whatever `PATH` names: these suites are integration tests against the
//! installed Git, and the version is what every measurement above was taken
//! against. **This door covers the commands this module builds, and not the
//! manager's.** `WorkspaceManager::command` composes its own environment and
//! sets neither `core.attributesFile` nor `GIT_ATTR_NOSYSTEM`, so an ambient
//! attributes file still rewrites what `git worktree add` checks out under
//! [`Fixture::add_task`]. That is the parent's command builder and a deferred
//! row against `standards/SWEEP.md` queue row 11, not a claim this module gets
//! to make.
//!
//! # What the repository itself pins
//!
//! Three settings live in the repository's own config rather than on this
//! module's commands, because they have to bind the manager's commands too:
//! `core.autocrlf=false` and `core.eol=lf`, and `core.fsmonitor=false`. The
//! object format is `--object-format` on `git init` ([`ObjectFormat`]) and the
//! initial branch is `-b main`, both arguments rather than settings.

use super::*;

// `OsStr` came from the parent's import list until the `m4-workspace` split
// moved its last production user into a child; named here for the same reason.
use std::cell::{Cell, RefCell};
use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;

// §8 names this token for exactly what `scratch_tree` does: recursive deletion
// of a run-scoped tree is token-carried, and the `cfg(test)` scratch-tree token
// is the one a test build carries.
use crate::rundir::scratch_tree::{ScratchTree, acquire};

// -----------------------------------------------------------------------
// Fixtures
// -----------------------------------------------------------------------

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

/// A scratch tree of this call's own, guarded, and reclaimed when the guard
/// drops.
///
/// **Through `rundir::scratch_tree`, which is the token §8 names**: recursive
/// deletion of a run-scoped tree is token-carried, and that module's whole
/// subject is this hazard. It replaces what stood here — a name built from a
/// tag, the process id and an ordinal, pre-cleaned with `remove_dir_all`
/// before anything was acquired — which after a process-id collision, or with
/// two process-id namespaces sharing one temporary directory, destroyed a tree
/// this process had no claim on. Creating exclusively afterwards did not
/// authorise the deletion that preceded it. Now the name carries a fresh ULID
/// so no two calls can collide, the root is created with an exclusive
/// `create_dir` so "previously nonexistent" is the kernel's answer, nothing is
/// pre-cleaned, and the removal is spent against a token bound to that exact
/// root.
///
/// # Panics
///
/// If `tag` is not a [`safe_component`], or if the acquisition refuses,
/// naming the root and what the filesystem said.
pub(crate) fn scratch_tree(tag: &str) -> ScratchTree {
    if let Err(why) = safe_component(tag) {
        panic!("the fixture's scratch tag `{tag}` is not one path component: {why}");
    }
    match acquire(&std::env::temp_dir(), tag) {
        Ok(tree) => tree,
        Err(refusal) => panic!(
            "the fixture could not acquire a scratch tree at {}: {:?}",
            refusal.root().display(),
            refusal.source()
        ),
    }
}

/// [`scratch_tree`]'s root, for the one caller that wants a directory and not
/// a fixture.
///
/// **The guard is spent here and the tree is not reclaimed.** A token cannot
/// be minted outside `rundir::scratch_tree`, so a caller holding only a path
/// has no authority to delete anything, and this hands back exactly that: an
/// exclusively created, unpredictably named directory that nothing will
/// remove. One directory per suite run, from the single test that calls this.
/// The guard-returning [`scratch_tree`] is what a fixture uses, and moving the
/// last caller onto it is a deferred row rather than an edit here, because the
/// caller is in `src/workspace_manager/tests.rs`, which another pull request
/// is repairing.
pub(crate) fn scratch(tag: &str) -> PathBuf {
    scratch_tree(tag).disarm().path().to_path_buf()
}

/// A path that does not exist, cannot be predicted, and is outside every
/// repository this module builds.
///
/// `core.hooksPath`, `core.attributesFile` and `core.excludesFile` point at
/// it, and so do `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `HOME` and
/// `XDG_CONFIG_HOME`. Git runs no hook from a path that does not exist, reads
/// an absent config, attributes or excludes file as empty, and finds no `~`
/// configuration under a home that is not there.
///
/// **It used to be a bare name joined to the command's own directory**, which
/// put it *inside the repository*: `<repo>/upstroke-fixture-absent` is an
/// ordinary filename a test can write or a branch can carry, and a file there
/// is a **loaded global configuration file**. Measured on git 2.43.0 under
/// exactly these pins: planting `[i18n] commitEncoding = ISO-8859-1` at that
/// name made `git config --show-origin --list` report it as loaded, which is
/// the door's own claim falsified by an ordinary `git add`. The path is now
/// absolute, under the temporary directory, and carries a fresh ULID, so no
/// test can name it and nothing creates it. One per process, because a
/// constant path shared by every process is predictable again.
fn absent() -> &'static Path {
    static ABSENT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ABSENT.get_or_init(|| {
        // **Two components, and the second is why this holds.** A path whose
        // parent exists is one Git will create: measured on git 2.43.0, `git
        // config --global i18n.commitEncoding ISO-8859-1` through the exported
        // helper creates the file `GIT_CONFIG_GLOBAL` names and exits 0, and
        // every later fixture command then loads it. With the parent absent
        // the same command refuses -- `could not lock config file …: No such
        // file or directory`, exit 255, nothing created -- while reads stay
        // silent and successful. Nothing creates the directory below, so the
        // guarantee is Git's rather than this module's discipline.
        std::env::temp_dir()
            .join(format!("upstroke-fixture-absent-{}", crate::ulid::ulid()))
            .join("absent")
    })
}

/// The whole of what a Git child inherits from this process.
///
/// [`git_command`] clears the environment and then copies these back, so the
/// list is an allowlist rather than a filter: a name that is not here cannot
/// reach Git however it is spelt, which is what makes the module doc's claim a
/// construction rather than an enumeration of the settings anybody thought of.
///
/// Unix needs one name. Windows needs the operating system's own plumbing:
/// `git.exe` and the tools it starts resolve through `PATH` and `PATHEXT`,
/// load system libraries relative to `SystemRoot`/`windir`, start a shell
/// through `COMSPEC`, and write scratch files under `TEMP`/`TMP`; Git for
/// Windows also reads its installation and per-user directories through
/// `USERPROFILE`, `LOCALAPPDATA`, `APPDATA` and `ProgramData`. None of them
/// carries configuration this module has not already neutralised: the config,
/// template and attributes sources are closed by the variables set below,
/// which win over anything these could point at.
#[cfg(unix)]
const INHERITED: [&str; 1] = ["PATH"];

/// See the Unix arm.
#[cfg(not(unix))]
const INHERITED: [&str; 12] = [
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "COMSPEC",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "ProgramData",
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
    let absent = absent();
    let mut command = Command::new("git");
    // The door. Everything this process holds goes, and only the names on the
    // allowlist come back, so no `GIT_*` variable and no pointer to a config,
    // template or attributes file survives unless it is set below.
    command.env_clear();
    for name in INHERITED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", absent)
        .env("GIT_CONFIG_SYSTEM", absent)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        // Empty rather than absent: measured on git 2.43.0, an empty value
        // copies no template at all and warns about nothing, and it beats both
        // `init.templateDir` and the built-in template directory.
        .env("GIT_TEMPLATE_DIR", "")
        .env("HOME", absent)
        .env("XDG_CONFIG_HOME", absent)
        .env("GIT_AUTHOR_NAME", COMMITTER_NAME)
        .env("GIT_AUTHOR_EMAIL", COMMITTER_EMAIL)
        .env("GIT_AUTHOR_DATE", COMMITTER_DATE)
        .env("GIT_COMMITTER_NAME", COMMITTER_NAME)
        .env("GIT_COMMITTER_EMAIL", COMMITTER_EMAIL)
        .env("GIT_COMMITTER_DATE", COMMITTER_DATE)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .arg("-C")
        .arg(dir);
    for key in ["core.hooksPath", "core.attributesFile", "core.excludesFile"] {
        let mut setting = OsString::from(key);
        setting.push("=");
        setting.push(absent);
        command.arg("-c").arg(setting);
    }
    command
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .stdin(Stdio::null());
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

/// Run `git` in `dir`, require it to succeed, and return its stdout with the
/// one line terminator Git ends an answer with removed, and nothing else.
///
/// **`trim()` is what this must not do**, and did: `rev-parse
/// --show-toplevel` in a repository whose path ends in a space answers with
/// that space and then the terminator, and trimming whitespace returns a
/// different path. Only `\n`, and the `\r\n` Git writes on Windows, come off.
///
/// The answer is decoded strictly rather than lossily, so this validates UTF-8
/// instead of assuming it: an identity — a commit, a ref name, a path — may
/// not be a replacement character where Git put a byte (§8).
///
/// # Panics
///
/// If the command fails, quoting both streams because Git reports on stdout as
/// often as on stderr (`git commit` with nothing staged says so on stdout and
/// exits 1). If its answer is not UTF-8, naming the byte index where it stops
/// being so.
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
        Ok(text) => text
            .strip_suffix('\n')
            .map_or(text.as_str(), |line| {
                line.strip_suffix('\r').unwrap_or(line)
            })
            .to_owned(),
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

/// How a [`Fixture`] reclaims its tree.
///
/// Two arms because the two cases have different authority. A fixture this
/// process built acquired its tree through §8's token and spends that token on
/// the way out. A fixture *adopted* from a tree another process built has no
/// token and cannot mint one, so it keeps master's behaviour: a best-effort
/// recursive removal of the root it was handed. That second arm is a §8
/// deviation this pull request does not close — see [`Fixture::adopt`].
enum Reclaim {
    /// Token-carried, for a tree this process acquired. Underscored because
    /// nothing reads the guard: it is held so that its own `Drop` spends the
    /// token.
    Owned(#[allow(dead_code)] ScratchTree),
    /// Master's, for a tree another process built and this one was handed.
    Adopted(PathBuf),
}

impl Drop for Reclaim {
    fn drop(&mut self) {
        match self {
            // The guard spends the token itself.
            Self::Owned(_) => {}
            // Best effort, exactly as master's `Fixture::drop` was: there is
            // no error path from a `Drop`, and a tree left behind is the
            // inode class this pull request rows rather than a silent success.
            Self::Adopted(root) => {
                let _ = fs::remove_dir_all(root);
            }
        }
    }
}

pub(crate) struct Fixture {
    /// The root of the scratch tree, as a field because six files read it that
    /// way and an accessor would edit `src/workspace_manager/tests.rs`, which
    /// another pull request is repairing. It is a copy of the guard's own path
    /// and never the authority for anything: the authority is the token.
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
    /// How the tree is reclaimed, and with what authority.
    ///
    /// **Last field, so it drops last** and the tree outlives everything that
    /// reads from it. Underscored because nothing reads it: it is held for its
    /// `Drop`.
    _reclaim: Reclaim,
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

    /// A repository of the given object format, under [`scratch_parent`].
    pub(crate) fn with_object_format(tag: &str, object_format: ObjectFormat) -> Self {
        let scratch = scratch_tree(tag);
        let root = scratch.path().to_path_buf();
        Self::at(root, Reclaim::Owned(scratch), object_format)
    }

    /// The body every constructor shares, over a root and whatever owns it.
    fn at(root: PathBuf, reclaim: Reclaim, object_format: ObjectFormat) -> Self {
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
            _reclaim: reclaim,
        }
    }

    /// Re-open a fixture a **previous process** built.
    ///
    /// **The adopted tree is reclaimed exactly as master reclaims it**, and
    /// that is a deliberate limit rather than an oversight. A token cannot be
    /// minted from a path, so this process cannot take ownership of a tree its
    /// child created. The shape that tried — the parent minting the tree and
    /// handing the child an environment variable naming it — was an
    /// unauthenticated protocol that reinitialised any repository the variable
    /// pointed at, and it is withdrawn. Leaving the tree behind instead would
    /// add a leak master does not have, on a box that has twice run out of
    /// inodes to exactly this class. So this arm keeps master's best-effort
    /// removal; the §8 deviation it carries is a deferred row, and closing it
    /// needs an ownership handoff somebody can authenticate.
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
    /// `seed` is the repository's root commit and not the parent of `main`.
    /// The field is documented as the first commit, and a child that committed
    /// on `main` before it died — which is what several of these tests have it
    /// do — moves that parent off it while leaving the root commit where it
    /// was.
    ///
    /// **`owner` is the guard the caller minted before it spawned the child**,
    /// and it is why nothing leaks. A token cannot be minted from a path, so
    /// this process cannot take ownership of a tree its child created; instead
    /// the caller acquires the tree first, hands the child [`SCRATCH_ROOT`],
    /// and passes the guard here. `root` is then a subtree of what `owner`
    /// names, and dropping this fixture reclaims both.
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
            base,
            private,
            manager,
            seed,
            head,
            side,
            // **Built last, in the struct literal, and that is the whole
            // point.** Every line above can fail — a stale or malformed
            // handoff, a repository without `main` or `side`, a `derive` that
            // refuses — and while this value existed before them, unwinding
            // dropped it and recursively deleted the root it was handed. That
            // deletes another tree when the handoff is wrong, and it destroys
            // the failed child's evidence exactly when somebody needs to look
            // at it. Master built the droppable `Fixture` only after these
            // steps succeeded; so does this.
            // `an_adoption_that_fails_validation_leaves_the_tree_alone` is the
            // witness, because construction order is invisible in review.
            _reclaim: Reclaim::Adopted(root.clone()),
            root,
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

/// What a Rust process exits with when a panic unwinds out of main.
const PANIC_EXIT: i32 = 101;

/// Run this test binary again, `--exact --ignored`, with `env` set, and
/// return its exit status.
///
/// The kill-test shape `src/rundir.rs` established: `Injection::Kill` is
/// `std::process::abort()`, a real process death, so the child has to be a
/// real process and the claim is what it left on disk. `env` is a list
/// rather than a map so a caller can pass the same key twice and see the
/// last win, exactly as `Command` does.
///
/// **This is master's, restored.** Three frontier passes found defects in the
/// capture that replaced it — unbounded buffering, no timeout, a child that
/// exits with a grandchild holding the pipe and a reader still attached — and
/// the third ruled that a fixture may not grow its own process containment.
/// What goes with the capture, and is named here rather than lost: the check
/// that the harness selected exactly one test. `--exact` against a name no
/// test has runs nothing and exits 0, which reads to a caller as a child that
/// completed, and that check had fixed a real misdiagnosis. It needs the
/// child's stdout, and the capture that reads it is what was withdrawn.
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
/// **A value per platform, and on Windows that used to be a negation.** It was
/// `!status.success() && status.code() != Some(PANIC_EXIT)`, which accepts
/// every unsuccessful exit that is not a panic — `std::process::exit(1)`
/// included — so a child that failed for any reason at all read as a child the
/// injection had killed, and the oracle had stopped discriminating. No Unix
/// run could see it: the Unix arm names `SIGABRT`, so the mutation is
/// invisible on every leg this box runs. Found by `sweep-tests-25` and routed
/// here because this file is `PR #135`'s subject.
///
/// Unix has one value, `SIGABRT`, which no Rust panic raises and which is a
/// name rather than a number. Windows has no portable one — `abort()` reaches
/// `__fastfail`, whose code has moved between CRT versions — and the answer is
/// **not** to write down the code this session believes it is today. Nobody
/// here has measured a Windows abort, and a constant nobody measured is the
/// defect this pull request has been finding all afternoon. So the Windows arm
/// *measures* instead: [`measured_abort_end`] runs a real
/// `std::process::abort()` once and remembers how it ended, and this compares
/// against that. A CRT that changes the code cannot make the oracle stale,
/// because the oracle asks the CRT. The design is `sweep-tests-25`'s, which
/// found this and built the comparison shape for its own suite.
///
/// `the_abort_oracle_separates_an_abort_from_an_ordinary_failure` runs both
/// children on **every** platform, so the arm each leg compiles is the arm
/// that leg measures.
pub(crate) fn died_by_abort(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(status) == Some(libc::SIGABRT)
    }
    #[cfg(windows)]
    {
        status.code().is_some() && status.code() == measured_abort_end()
    }
}

/// How a real `std::process::abort()` ends on this machine, measured once.
///
/// Windows only, because Unix names `SIGABRT` and needs no measurement. One
/// child per test process, cached: [`died_by_abort`] is called once per kill
/// test and this is the first of them.
///
/// # Panics
///
/// Through [`run_kill_child`], if the probe cannot be run or does not run its
/// one test — a probe that silently stopped aborting would otherwise make the
/// oracle accept whatever the probe returned instead.
#[cfg(windows)]
fn measured_abort_end() -> Option<i32> {
    static END: std::sync::OnceLock<Option<i32>> = std::sync::OnceLock::new();
    *END.get_or_init(|| {
        let probe = run_kill_child(ABORT_PROBE, &[(ABORT_PROBE_ARM, OsStr::new("1"))]);
        assert!(
            !probe.success(),
            "the abort probe exited successfully, so it did not abort: {probe:?}"
        );
        probe.code()
    })
}

/// The harness name of the child that aborts.
pub(crate) const ABORT_PROBE: &str = "workspace_manager::fixture::tests::aborting_child";

/// What arms [`ABORT_PROBE`].
///
/// The child returns without aborting unless this is set, so a run with
/// `--include-ignored` cannot kill the suite with it.
pub(crate) const ABORT_PROBE_ARM: &str = "UPSTROKE_TEST_ABORT_PROBE";

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
    /// child. `run_kill_child` refuses a name no test has, so a rename here
    /// fails loudly rather than passing vacuously.
    const AMBIENT_CHILD: &str = "workspace_manager::fixture::tests::ambient_environment_child";

    /// The harness name of [`exiting_child`].
    const EXITING_CHILD: &str = "workspace_manager::fixture::tests::exiting_child";

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

    /// The pre-clean is gone, and what replaces it never touches a tree it did
    /// not create: two acquisitions are two roots, and the first one's bytes
    /// are still there after the second.
    #[test]
    fn a_second_scratch_tree_is_a_second_root_and_pre_cleans_nothing() {
        let first = scratch_tree("token-first");
        let planted = first.path().join("planted.txt");
        write_file(&planted, b"the first tree's own bytes\n");

        let second = scratch_tree("token-second");
        assert_ne!(
            first.path(),
            second.path(),
            "a fresh ULID means two acquisitions cannot collide on a name"
        );
        assert!(
            planted.exists(),
            "the second acquisition removed the first tree's bytes at {}",
            planted.display()
        );
    }

    /// The guard reclaims **its own** root and nothing above or beside it, and
    /// it reclaims on drop rather than at the end of a passing body, so a
    /// failing test leaves no tree behind.
    #[test]
    fn a_dropped_scratch_guard_reclaims_its_own_tree_and_nothing_else() {
        let neighbour = scratch_tree("token-neighbour");
        let beside = neighbour.path().join("beside.txt");
        write_file(&beside, b"a neighbouring tree\n");

        let root = {
            let tree = scratch_tree("token-dropped");
            let root = tree.path().to_path_buf();
            write_file(&root.join("inside.txt"), b"inside the guarded tree\n");
            root
        };

        assert!(
            !root.exists(),
            "the guard's own tree survived its drop: {}",
            root.display()
        );
        assert!(
            beside.exists(),
            "the drop reached outside the token's root, to {}",
            beside.display()
        );
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

        let status = run_kill_child(
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

    /// The door, asked of Git rather than of the constant: `config
    /// --show-origin --list` names every source a command actually read, so a
    /// global, a system or a template-installed config would appear here.
    ///
    /// This is the check that replaces enumerating variables. A list of names
    /// to strip can only be as complete as whoever wrote it; this asserts the
    /// property — the only configuration a fixture command sees is the
    /// repository's own and this module's own `-c` — and it holds for a
    /// setting nobody has thought of yet.
    #[test]
    fn the_only_config_a_fixture_command_reads_is_the_repositorys_own() {
        let fixture = Fixture::new("config-origins");
        // The pass-2 reviewer's own reproduction: the pins used to name a bare
        // filename joined to the command's directory, so a file at that
        // ordinary name inside the repository was read as a loaded global
        // configuration file. Planted here as valid Git config that would
        // change every commit id if it were loaded.
        write_file(
            &fixture.base.join("upstroke-fixture-absent"),
            b"[i18n]\n\tcommitEncoding = ISO-8859-1\n",
        );
        let listed = git(&fixture.base, &["config", "--show-origin", "--list"]);
        let mut origins: Vec<&str> = listed
            .lines()
            .filter_map(|line| line.split_once('\t').map(|(origin, _)| origin))
            .collect();
        origins.sort_unstable();
        origins.dedup();
        assert!(
            !origins.is_empty(),
            "the probe read no configuration at all, so it is measuring nothing"
        );
        // Git spells a `file:` origin relative to the command's own directory,
        // which `-C` made the repository, so each one is resolved against it
        // and compared as a path rather than as text.
        let repository = fixture
            .base
            .join(".git")
            .join("config")
            .canonicalize()
            .expect("canonicalize the repository's own config");
        let foreign: Vec<&&str> = origins
            .iter()
            .filter(|origin| {
                if origin.starts_with("command line:") {
                    return false;
                }
                origin.strip_prefix("file:").is_none_or(|named| {
                    fixture
                        .base
                        .join(named)
                        .canonicalize()
                        .is_ok_and(|path| path != repository)
                })
            })
            .collect();
        assert!(
            foreign.is_empty(),
            "a fixture command read configuration from outside its own repository: {foreign:?}"
        );
    }

    /// The other half of the ambient claim, and the shape the first frontier
    /// pass attacked: a **configuration file and a template directory** this
    /// process cannot replace for itself.
    ///
    /// The child is given a global and a system config file, and a template
    /// directory, carrying every escape the pass found and the ones the sweep
    /// had already pinned: signing on, another default branch, `autocrlf`, an
    /// fsmonitor, an ignore file that excludes the seed files, an attributes
    /// file and a template `info/attributes` that rewrite line endings on
    /// checkout, a hooks directory and a template `post-checkout`, an
    /// `i18n.commitEncoding` that puts an `encoding` header on every commit and
    /// so changes every object id, and a template `config` setting
    /// `core.worktree` to another directory — which, unclosed, makes the new
    /// repository resolve its worktree to that one and stage its files
    /// (measured, git 2.43.0).
    ///
    /// It asserts the property rather than the pins: the child's three commit
    /// ids, its file content, its own worktree, and no hook.
    #[test]
    fn the_fixture_is_immune_to_an_ambient_config_and_template() {
        let clean = Fixture::new("config-clean");
        let expected = format!("{} {} {}", clean.seed, clean.head, clean.side);

        let hostile = clean.root.join("hostile");
        let excludes = hostile.join("ignore");
        let attributes = hostile.join("attributes");
        let hooks = hostile.join("hooks");
        let hook = hooks.join("post-checkout");
        let template = hostile.join("template");
        let elsewhere = hostile.join("elsewhere");
        write_file(&excludes, b"*.txt\n");
        write_file(&attributes, b"* text eol=crlf\n");
        write_file(&hook, b"#!/bin/sh\n: > hook-ran.marker\n");
        write_file(
            &template.join("info").join("attributes"),
            b"* text eol=crlf\n",
        );
        write_file(
            &template.join("hooks").join("post-checkout"),
            b"#!/bin/sh\n: > hook-ran.marker\n",
        );
        create_dir(&elsewhere);
        let mut template_config = b"[core]\n\tworktree = ".to_vec();
        template_config.extend_from_slice(&config_path_bytes(&elsewhere));
        template_config.push(b'\n');
        write_file(&template.join("config"), &template_config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for script in [&hook, &template.join("hooks").join("post-checkout")] {
                fs::set_permissions(script, fs::Permissions::from_mode(0o755))
                    .expect("make the hostile hook executable");
            }
        }

        let mut config = b"[commit]\n\tgpgsign = true\n[init]\n\tdefaultBranch = trunk\n".to_vec();
        config.extend_from_slice(
            b"[i18n]\n\tcommitEncoding = ISO-8859-1\n[core]\n\tautocrlf = true\n",
        );
        for (key, value) in [
            ("\tfsmonitor = ", &hook),
            ("\texcludesFile = ", &excludes),
            ("\tattributesFile = ", &attributes),
            ("\thooksPath = ", &hooks),
        ] {
            config.extend_from_slice(key.as_bytes());
            config.extend_from_slice(&config_path_bytes(value));
            config.push(b'\n');
        }
        config.extend_from_slice(b"[init]\n\ttemplateDir = ");
        config.extend_from_slice(&config_path_bytes(&template));
        config.push(b'\n');
        let config_path = hostile.join("config");
        write_file(&config_path, &config);

        let status = run_kill_child(
            CONFIG_CHILD,
            &[
                (AMBIENT_EXPECT, OsStr::new(expected.as_str())),
                ("GIT_CONFIG_GLOBAL", config_path.as_os_str()),
                ("GIT_CONFIG_SYSTEM", config_path.as_os_str()),
                ("GIT_TEMPLATE_DIR", template.as_os_str()),
            ],
        );
        assert!(
            status.success(),
            "the child built its fixture under a hostile Git configuration and template and \
             ended {status:?}"
        );
    }

    /// `path` as the bytes a Git config value spells it with.
    ///
    /// A display conversion is for diagnostics (§8), and this value is written
    /// into a file Git then reads as a path, so the conversion is exact where
    /// it can be and a refusal where it cannot: on Unix the path's own bytes,
    /// and elsewhere a path that is not Unicode is refused rather than having
    /// its bytes replaced, because a Git config file there is UTF-8 and a
    /// replacement character is a different path. Git's parser reads `\\` as an
    /// escape, so each one is doubled — on the bytes, not on a `String`.
    fn config_path_bytes(path: &Path) -> Vec<u8> {
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt as _;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let bytes = match path.to_str() {
            Some(text) => text.as_bytes().to_vec(),
            None => panic!(
                "the fixture cannot write {} into a Git config file: it is not Unicode",
                path.display()
            ),
        };
        let mut escaped = Vec::with_capacity(bytes.len());
        for byte in bytes {
            if byte == b'\\' {
                escaped.push(b'\\');
            }
            escaped.push(byte);
        }
        escaped
    }

    /// `trim()` ate a trailing space from a path Git answered with, which is a
    /// different path. Unix only: Windows strips a trailing space from a
    /// directory name itself, so the case cannot be built there.
    #[cfg(unix)]
    #[test]
    fn git_strips_the_line_terminator_and_not_a_trailing_space() {
        let tree = scratch_tree("trailing-space");
        let awkward = tree.path().join("repo with a trailing space ");
        create_dir(&awkward);
        git(&awkward, &["init", "-q", "-b", "main"]);
        assert_eq!(
            PathBuf::from(git(&awkward, &["rev-parse", "--show-toplevel"])),
            awkward
                .canonicalize()
                .expect("canonicalize the awkwardly named repository"),
            "the trailing space came off with the line terminator"
        );
    }

    /// The oracle itself, run on **every** platform, because the arm a leg
    /// compiles is the only arm that leg can measure: the Unix arm names
    /// `SIGABRT` and cannot see the Windows arm's defect, which is why this
    /// one was found by another session's Windows work rather than by any run
    /// on this box.
    ///
    /// Two children: one dies by `std::process::abort()`, one exits non-zero
    /// the ordinary way. The oracle must say yes to the first and no to the
    /// second. The negation it replaces said yes to both.
    #[test]
    fn the_abort_oracle_separates_an_abort_from_an_ordinary_failure() {
        let aborted = run_kill_child(ABORT_PROBE, &[(ABORT_PROBE_ARM, OsStr::new("1"))]);
        let exited = run_kill_child(EXITING_CHILD, &[]);

        // Premises first, so a probe that stopped working is loud rather than
        // vacuous.
        assert!(
            died_by_abort(&aborted),
            "the oracle did not recognise a real `std::process::abort()`: {aborted:?}"
        );
        assert_eq!(
            exited.code(),
            Some(1),
            "the ordinary-failure child did not exit 1: {exited:?}"
        );

        // The defect, written out rather than described: the negation this
        // replaced accepts both children, so it is not an abort oracle at all.
        // This assertion is what fails first if the negation ever comes back.
        let negation = |status: &std::process::ExitStatus| {
            !status.success() && status.code() != Some(PANIC_EXIT)
        };
        assert!(
            negation(&aborted) && negation(&exited),
            "the negation is supposed to accept both, which is why it was replaced"
        );

        assert!(
            !died_by_abort(&exited),
            "the oracle read an ordinary non-zero exit as an abort: {exited:?}"
        );
        assert!(
            !died_by_abort(&exited_with(PANIC_EXIT)),
            "the oracle read a panic as an abort"
        );
        assert!(
            !same_end(&aborted, &exited),
            "an abort and an exit of one are not the same end"
        );
        assert!(
            same_end(&aborted, &aborted),
            "a comparison that rejects everything would satisfy the line above"
        );
    }

    /// Whether two children ended the same way, compared as values.
    ///
    /// `sweep-tests-25`'s shape: not "both unsuccessful", which is the
    /// negation being replaced.
    fn same_end(left: &std::process::ExitStatus, right: &std::process::ExitStatus) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            left.signal() == right.signal() && left.code() == right.code()
        }
        #[cfg(not(unix))]
        {
            left.code() == right.code()
        }
    }

    /// A status for a process that exited with `code` and no signal.
    fn exited_with(code: i32) -> std::process::ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            // A `wait` status packs the exit code into the high byte.
            std::process::ExitStatus::from_raw(code << 8)
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::process::ExitStatusExt as _;
            std::process::ExitStatus::from_raw(code as u32)
        }
    }

    /// The abort half of [`the_abort_oracle_separates_an_abort_from_an_ordinary_failure`],
    /// and the probe [`died_by_abort`] measures against on Windows.
    ///
    /// Armed by [`ABORT_PROBE_ARM`] and inert without it, so a run that
    /// includes ignored tests does not abort the suite.
    #[test]
    #[ignore = "spawned by `the_abort_oracle_separates_an_abort_from_an_ordinary_failure`"]
    fn aborting_child() {
        if std::env::var_os(ABORT_PROBE_ARM).is_none() {
            return;
        }
        std::process::abort();
    }

    /// The ordinary-failure half: a non-zero exit that is not a panic and not
    /// an abort, which is what the Windows arm used to accept.
    #[test]
    #[ignore = "spawned by `the_abort_oracle_separates_an_abort_from_an_ordinary_failure`"]
    fn exiting_child() {
        std::process::exit(1);
    }

    /// An adopted fixture reclaims the tree it adopted, which is master's
    /// behaviour and is kept deliberately.
    ///
    /// `std::mem::forget` is the aborting child, exactly: its `Drop` never
    /// runs, so its tree survives it. The pull request that tried to give this
    /// process a *token* for that tree built an unauthenticated protocol and
    /// withdrew it; what is left is master's best-effort removal, and the
    /// alternative — reclaiming nothing — would add a leak master does not
    /// have. The tree must still be there while the adopting fixture is alive,
    /// or the removal has eaten the repository it is about to read.
    #[test]
    fn an_adopted_fixture_reclaims_the_tree_it_adopted() {
        let fixture = Fixture::new("adopted");
        let root = fixture.root.clone();
        // The child died by `abort()`: nothing of its own is reclaimed.
        std::mem::forget(fixture);
        assert!(
            root.exists(),
            "the abandoned tree is the premise of this test"
        );

        let adopted = Fixture::adopt(root.clone());
        assert!(
            root.exists(),
            "the tree was reclaimed while the fixture adopting it was still alive: {}",
            root.display()
        );
        assert_eq!(
            adopted.seed,
            git(&adopted.base, &["rev-list", "--max-parents=0", "main"]),
            "the adopted fixture can still read the repository it adopted"
        );
        drop(adopted);

        assert!(
            !root.exists(),
            "the tree the child left behind outlived the fixture that adopted it: {}",
            root.display()
        );
    }

    /// An adoption that fails validation must leave the tree alone.
    ///
    /// Construction order is invisible in review and no existing test
    /// exercised it, which is how `Reclaim::Adopted` came to be built before
    /// any of the four validation steps: an unwind then deleted the root the
    /// caller supplied, so a stale or malformed handoff destroyed another tree
    /// and a failed child's evidence went with it. This fails at `1bcc12e` and
    /// passes here.
    #[test]
    fn an_adoption_that_fails_validation_leaves_the_tree_alone() {
        let tree = scratch_tree("adopt-refused");
        let root = tree.path().to_path_buf();
        // A tree that is not a fixture: `adopt` reaches `rev-parse main` and
        // fails there, which is what a stale handoff looks like.
        write_file(
            &root.join("repo").join("evidence.txt"),
            b"a failed child's tree\n",
        );

        let refused = std::panic::catch_unwind({
            let root = root.clone();
            move || Fixture::adopt(root)
        });
        assert!(
            refused.is_err(),
            "adopting a tree that is not a repository must fail"
        );
        assert!(
            root.join("repo").join("evidence.txt").exists(),
            "the failed adoption deleted the tree it was handed, and the evidence with it: {}",
            root.display()
        );
    }

    /// The absent path the pins name is one Git will not create.
    ///
    /// The pass-3 reviewer's own sequence: a test can reach the exported `git`
    /// and run `config --global`, and Git creates whatever
    /// `GIT_CONFIG_GLOBAL` names without the caller knowing the randomised
    /// name. It only holds because the path's parent does not exist either.
    #[test]
    fn the_absent_path_is_one_git_refuses_to_create() {
        let fixture = Fixture::new("absent-write");
        let attempt = git_out(
            &fixture.base,
            &["config", "--global", "i18n.commitEncoding", "ISO-8859-1"],
        );
        assert!(
            !attempt.status.success(),
            "Git wrote a global config file through the fixture's own pins: {}",
            String::from_utf8_lossy(&attempt.stderr)
        );
        // And the door still holds afterwards: nothing outside the repository.
        let listed = git(&fixture.base, &["config", "--show-origin", "--list"]);
        let repository = fixture
            .base
            .join(".git")
            .join("config")
            .canonicalize()
            .expect("canonicalize the repository's own config");
        for origin in listed
            .lines()
            .filter_map(|line| line.split_once('\t').map(|(origin, _)| origin))
        {
            if origin.starts_with("command line:") {
                continue;
            }
            let named = origin.strip_prefix("file:").unwrap_or(origin);
            assert_eq!(
                fixture.base.join(named).canonicalize().ok(),
                Some(repository.clone()),
                "a fixture command read configuration from {origin}"
            );
        }
    }

    /// The child half of [`the_fixture_is_immune_to_an_ambient_config_and_template`].
    #[test]
    #[ignore = "spawned by `the_fixture_is_immune_to_an_ambient_config_and_template` with a \
                hostile Git configuration and template"]
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
        //
        // **This is the fixture's own checkout and says nothing about the
        // manager's.** `WorkspaceManager::command` sets neither
        // `core.attributesFile` nor `GIT_ATTR_NOSYSTEM`, so an ambient
        // attributes file still rewrites what `git worktree add` writes under
        // `Fixture::add_task`. That is the parent's command builder and a
        // deferred row against queue row 11; nothing here covers it, and the
        // module doc says so rather than letting this assertion imply it.
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
            "an ambient hooks path, or a template hook, ran during the fixture's own checkouts"
        );
        // Canonicalised on both sides: Git answers with forward slashes and no
        // verbatim prefix, and `canonicalize` on Windows answers `\\?\C:\...`,
        // so comparing the two spellings fails on the guest for a repository
        // that is exactly where it should be (measured, winguest).
        assert_eq!(
            PathBuf::from(git(&fixture.base, &["rev-parse", "--show-toplevel"]))
                .canonicalize()
                .expect("canonicalize the worktree Git reported"),
            fixture
                .base
                .canonicalize()
                .expect("canonicalize the fixture's own repository"),
            "a template `core.worktree` moved the repository's worktree elsewhere"
        );
    }
}
