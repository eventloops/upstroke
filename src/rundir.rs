//! Run directory layout (DESIGN.md §15) and run discovery.
//!
//! §15 draws the whole run directory under `.tactus/runs/<run-id>/`. This
//! module splits it in two, and the reason is enforcement rather than tidiness.
//!
//! A reviewer is a read-only agent pointed at the workspace, so every path
//! inside the workspace is reachable — including, before this split, the
//! implementer's own transcript. Invariant 3 says the diff is ground truth and
//! the transcript is not, so a reviewer reading the transcript is judging the
//! wrong evidence. Permission deny rules cannot close that on their own: gates
//! execute repository code the implementer just wrote, and that code reads any
//! workspace path the deny list never sees.
//!
//! So the split follows what each file is *for*. The ops surface — what
//! `status`, `resume`, `answer`, and any future pane read — stays in the repo
//! where §15 documents it and where CI can collect it. The agent-authored text
//! moves to a user-level directory no sandboxed agent has a path into. The
//! `run_started` event records where that directory is, so the record stays
//! self-describing rather than depending on this function's defaults.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::TactusError;
use crate::util;

/// Created beside the repo: the run's own record and the human/UI surface.
const PUBLIC_DIRS: [&str; 3] = ["artifacts", "questions", "answers"];
/// Created outside the workspace: everything an agent wrote or that describes
/// an agent's sandbox.
const PRIVATE_DIRS: [&str; 4] = ["transcripts", "reviews", "settings", "gates"];

/// Where one run's files live, split by who is allowed to read them.
#[derive(Debug, Clone)]
pub struct RunPaths {
    /// `<repo>/.tactus/runs/<run-id>` — `events.jsonl`, the frozen plan,
    /// artifacts, questions, answers, the lock. Git-ignored, but present
    /// beside the repository it describes.
    pub public: PathBuf,
    /// `~/.tactus/runs/<run-id>` — transcripts, review verdicts, gate logs,
    /// and the per-attempt permission settings that define each sandbox.
    pub private: PathBuf,
}

impl RunPaths {
    /// Layout for a fresh run, with the private half at its default root.
    pub fn new(repo_root: &Path, run_id: &str) -> Self {
        Self::with_private_root(repo_root, run_id, &default_private_root())
    }

    /// Layout with an explicit private root — how tests stay out of the real
    /// `~/.tactus`, and how a caller pins the location deliberately.
    pub fn with_private_root(repo_root: &Path, run_id: &str, private_root: &Path) -> Self {
        Self {
            public: public_dir(repo_root, run_id),
            private: private_root.join("runs").join(run_id),
        }
    }

    /// Rebuild from a private directory recorded in `run_started`. Resume and
    /// status use this so they read the run that actually happened rather than
    /// wherever today's defaults would have put it.
    pub fn from_parts(public: PathBuf, private: PathBuf) -> Self {
        Self { public, private }
    }

    /// Create both trees. Callers do this once at run start; every accessor
    /// below assumes it has happened.
    pub fn create(&self) -> Result<(), TactusError> {
        let dirs = PUBLIC_DIRS
            .iter()
            .map(|name| self.public.join(name))
            .chain(PRIVATE_DIRS.iter().map(|name| self.private.join(name)));
        for dir in dirs {
            fs::create_dir_all(&dir).map_err(|source| TactusError::Io {
                path: dir.clone(),
                source,
            })?;
        }
        Ok(())
    }

    /// The append-only source of truth (§15).
    pub fn events(&self) -> PathBuf {
        self.public.join("events.jsonl")
    }

    /// The frozen plan this run is executing (§5).
    pub fn plan_json(&self) -> PathBuf {
        self.public.join("plan.normalized.json")
    }

    /// A projection of the log for humans and tooling — derived, never read
    /// back as state.
    pub fn report_json(&self) -> PathBuf {
        self.public.join("report.json")
    }

    /// Held for the lifetime of a run so two engines cannot drive one branch.
    pub fn lock_file(&self) -> PathBuf {
        lock_file(&self.public)
    }

    pub fn questions(&self) -> PathBuf {
        self.public.join("questions")
    }

    /// Where `tactus answer` drops an answer for the engine to ingest.
    pub fn answers(&self) -> PathBuf {
        self.public.join("answers")
    }

    pub fn artifacts(&self) -> PathBuf {
        self.public.join("artifacts")
    }

    pub fn transcripts(&self) -> PathBuf {
        self.private.join("transcripts")
    }

    pub fn reviews(&self) -> PathBuf {
        self.private.join("reviews")
    }

    pub fn settings(&self) -> PathBuf {
        self.private.join("settings")
    }

    pub fn gates(&self) -> PathBuf {
        self.private.join("gates")
    }
}

/// `~/.tactus`, or a temp-dir equivalent when no home resolves.
///
/// The fallback is deliberately still outside the workspace. Falling back to
/// the repo would keep runs working on a machine with no `HOME` while silently
/// dropping the isolation this module exists for — a security property that
/// degrades quietly is worse than one that was never claimed.
fn default_private_root() -> PathBuf {
    util::user_tactus_dir().unwrap_or_else(|| std::env::temp_dir().join("tactus"))
}

/// `<repo>/.tactus/runs/<run-id>` — §15's documented location.
pub fn public_dir(repo_root: &Path, run_id: &str) -> PathBuf {
    runs_root(repo_root).join(run_id)
}

pub fn runs_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".tactus").join("runs")
}

/// Every run in this repo, oldest first.
///
/// Run ids are ULIDs with the millisecond timestamp in the high bits and
/// Crockford base32's digits-before-letters ordering, so a plain lexicographic
/// sort is chronological — no directory timestamps, which copying a repo would
/// scramble.
pub fn list_runs(repo_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(runs_root(repo_root)) else {
        return Vec::new();
    };
    let mut runs: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    runs.sort();
    runs
}

/// The most recent run — what `tactus status` reports when given no id.
pub fn latest_run(repo_root: &Path) -> Option<String> {
    list_runs(repo_root).pop()
}

/// Resolve a run id from any unambiguous prefix, so an operator can type the
/// first few characters of a 26-character ULID.
///
/// An exact match wins outright rather than being treated as one candidate
/// among several: a full id is never ambiguous, even if some other run happens
/// to extend it.
pub fn resolve_run_id(repo_root: &Path, wanted: &str) -> Result<String, TactusError> {
    let runs = list_runs(repo_root);
    let wanted_upper = wanted.to_ascii_uppercase();
    // The entry as it exists on disk, not the uppercased input. The comparison
    // is case-insensitive because a run directory can arrive from a
    // case-insensitive filesystem, and on a case-sensitive one only the real
    // name builds a path that opens — everything downstream joins this id.
    if let Some(matched) = runs.iter().find(|id| id.eq_ignore_ascii_case(wanted)) {
        return Ok(matched.clone());
    }
    let matches: Vec<&String> = runs
        .iter()
        .filter(|id| id.to_ascii_uppercase().starts_with(&wanted_upper))
        .collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(TactusError::Refused {
            message: if runs.is_empty() {
                format!("no runs found under {}", runs_root(repo_root).display())
            } else {
                format!("no run matches that id; known runs: {}", runs.join(", "))
            },
        }),
        several => Err(TactusError::Refused {
            message: format!(
                "that prefix matches {} runs ({}); use more characters",
                several.len(),
                several
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

/// A question id resolved to the run that raised it.
#[derive(Debug)]
pub struct FoundQuestion {
    pub run_id: String,
    /// The run's public directory — everything `tactus answer` touches.
    pub public: PathBuf,
    /// The full question id, expanded from whatever prefix was typed.
    pub question_id: String,
}

/// Find the run holding a question, by full id or unambiguous prefix.
///
/// Scans every run rather than requiring the operator to remember which one
/// asked: the notifier hands them a question id, not a run id, so a question
/// id is what the command has to accept.
pub fn find_question(repo_root: &Path, wanted: &str) -> Result<FoundQuestion, TactusError> {
    let wanted_upper = wanted.to_ascii_uppercase();
    let mut exact: Option<FoundQuestion> = None;
    let mut matches: Vec<FoundQuestion> = Vec::new();
    for run_id in list_runs(repo_root) {
        let public = public_dir(repo_root, &run_id);
        let Ok(entries) = fs::read_dir(public.join("questions")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(question_id) = name.strip_suffix(".json") else {
                continue;
            };
            let found = FoundQuestion {
                run_id: run_id.clone(),
                public: public.clone(),
                question_id: question_id.to_owned(),
            };
            if question_id.eq_ignore_ascii_case(wanted) {
                exact = Some(found);
            } else if question_id.to_ascii_uppercase().starts_with(&wanted_upper) {
                matches.push(found);
            }
        }
    }
    if let Some(found) = exact {
        return Ok(found);
    }
    match matches.len() {
        1 => matches.pop().ok_or_else(|| TactusError::Refused {
            message: "question vanished while resolving it".to_owned(),
        }),
        0 => Err(TactusError::Refused {
            message: format!(
                "no question with that id under {}",
                runs_root(repo_root).display()
            ),
        }),
        several => Err(TactusError::Refused {
            message: format!(
                "that prefix matches {several} questions ({}); use more characters",
                matches
                    .iter()
                    .map(|found| found.question_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

/// The lock beside one run's ops surface.
///
/// Takes the public directory rather than a whole [`RunPaths`] because the
/// lock lives in the public half by construction. Two callers only ever want
/// to know whether a run is live — `tactus answer`, and the resume that must
/// claim the run *before* it has read where the private half went — and
/// neither has a private path to offer. Asking them for one invited passing
/// the public path twice, which would have quietly become wrong the moment
/// liveness consulted anything but the lock.
pub fn lock_file(public: &Path) -> PathBuf {
    public.join("run.lock")
}

/// A second, Unix-only lock used as a crash-cleanup lease. Each external agent
/// reaper opens its own shared hold; `resume` needs the exclusive side, so a
/// hard-killed conductor cannot hand over the run before cleanup is complete.
#[cfg(unix)]
fn cleanup_lock_file(public: &Path) -> PathBuf {
    public.join("cleanup.lock")
}

/// An exclusive hold on one run, released when this value drops.
///
/// Two engines on one run directory would interleave events into the log and
/// fight over the same git branch and working tree. An advisory OS lock is the
/// right shape for that because the operating system releases the primary
/// hold when the conductor dies. On Unix, live crash reapers retain only the
/// shared cleanup lease until their agent groups are quiescent. Neither hold
/// leaves a stale marker to clear by hand.
///
/// Which OS lock, though, is not a detail. See [`imp`].
#[derive(Debug)]
pub struct RunLock {
    _file: File,
    _cleanup: cleanup::CleanupLease,
    /// The run this claimed in [`claims`], given back on drop.
    claim: PathBuf,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        // Before the file closes, so no window exists where the OS has let go
        // and this process still thinks it holds the run.
        claims().remove(&self.claim);
    }
}

impl RunLock {
    /// Take the lock on a run's public directory, or explain who has it.
    pub fn acquire(public: &Path) -> Result<Self, TactusError> {
        let path = lock_file(public);
        let claim = claim_key(public);
        // This process first, and not only as an optimisation: the OS lock
        // below is per-*process*, so it cannot tell one thread here from
        // another. `claims` is what makes two `acquire`s in one process behave
        // the way two engines do, and it is exact rather than advisory.
        if !claims().insert(claim.clone()) {
            return Err(refused(public, &path, Some(std::process::id())));
        }
        let taken = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&path)
            .map_err(|source| TactusError::Io {
                path: path.clone(),
                source,
            })
            .and_then(|file| match imp::take(&file) {
                Holder::Nobody => Ok(file),
                Holder::Someone { pid } => Err(refused(public, &path, pid)),
                // A lock that cannot be taken is not a lock that was taken. Say
                // what actually failed rather than blaming an engine that may
                // not exist.
                Holder::Unknown(source) => Err(TactusError::Io {
                    path: path.clone(),
                    source,
                }),
            });
        match taken {
            Ok(file) => match cleanup::take(public) {
                Ok(cleanup) => Ok(Self {
                    _file: file,
                    _cleanup: cleanup,
                    claim,
                }),
                Err(error) => {
                    claims().remove(&claim);
                    Err(error)
                }
            },
            Err(error) => {
                claims().remove(&claim);
                Err(error)
            }
        }
    }

    /// Bind subprocess cleanup started on this thread to this run.
    ///
    /// The lock itself remains `Send`; callers enter the scope only while
    /// synchronously driving the run, so a future executor can move ownership
    /// first and establish the context on its actual worker thread.
    pub(crate) fn enter_cleanup_scope(&self) -> cleanup::CleanupScope<'_> {
        cleanup::enter(&self._cleanup)
    }
}

fn refused(public: &Path, path: &Path, pid: Option<u32>) -> TactusError {
    let who = match pid {
        Some(pid) => format!(" (pid {pid})"),
        None => String::new(),
    };
    TactusError::Refused {
        message: format!(
            "another tactus process{who} is already driving run `{}` (lock held on {}). Two \
             engines would interleave events and fight over the same branch — wait for it to \
             finish, or stop it first.",
            public.file_name().unwrap_or_default().to_string_lossy(),
            path.display()
        ),
    }
}

/// Who holds a run's lock.
#[derive(Debug)]
enum Holder {
    Nobody,
    /// Somebody does. `pid` where the platform will say.
    Someone {
        pid: Option<u32>,
    },
    /// The call failed without answering the question.
    Unknown(io::Error),
}

/// Runs this process holds, so that two `acquire`s here behave like two
/// engines.
///
/// It also keeps [`is_running`] away from a lock file this process already
/// holds — which on Unix is not tidiness but a correctness requirement, because
/// closing *any* descriptor for a file releases every `fcntl` lock this process
/// has on it. A bare `File::open` + drop in the holder would silently hand the
/// run away. Answering from here means that open never happens.
fn claims() -> &'static Claims {
    static CLAIMS: Claims = Claims {
        runs: Mutex::new(BTreeSet::new()),
    };
    &CLAIMS
}

#[derive(Debug)]
struct Claims {
    runs: Mutex<BTreeSet<PathBuf>>,
}

impl Claims {
    /// `true` if this process did not already hold it.
    fn insert(&self, key: PathBuf) -> bool {
        self.held().insert(key)
    }

    fn remove(&self, key: &Path) {
        self.held().remove(key);
    }

    fn contains(&self, key: &Path) -> bool {
        self.held().contains(key)
    }

    /// A panic in a lock holder must not take the run lock's bookkeeping with
    /// it: the set is still exactly as valid as it was before the panic.
    fn held(&self) -> std::sync::MutexGuard<'_, BTreeSet<PathBuf>> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The run's identity for [`claims`], resolved so that two spellings of one
/// directory cannot look like two runs.
fn claim_key(public: &Path) -> PathBuf {
    fs::canonicalize(public).unwrap_or_else(|_| public.to_path_buf())
}

/// Whether a run is being driven right now, without disturbing the holder.
///
/// Read-only with respect to the run record. On Unix, `F_GETLK` asks who holds
/// the primary lock without taking one. Only when that lock is free does the
/// probe momentarily try the exclusive side of the cleanup lease; it never
/// creates or changes either file. A primary file that does not exist means
/// the run never started.
pub fn is_running(public: &Path) -> bool {
    // Asked and answered without touching the file. On Unix this is the branch
    // that keeps `fcntl`'s release-on-any-close from applying to us at all.
    if claims().contains(&claim_key(public)) {
        return true;
    }
    let file = match File::open(lock_file(public)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
        // An existing run whose lock cannot be inspected is not safe to call
        // dead. `acquire` will report the concrete IO error if a resume tries.
        Err(_) => return true,
    };
    match imp::holder(&file) {
        Holder::Nobody => cleanup::is_held(public),
        Holder::Someone { .. } => true,
        // The opened-fine-but-cannot-be-locked case, which is not the same as
        // the unopenable file above and does not get the same answer. Locking
        // fails with `ENOLCK` or `EOPNOTSUPP` on filesystems that do not carry
        // locks — NFS, SMB, some container overlays — and it does so whether or
        // not an engine is driving the run.
        //
        // So the question is which way to be wrong when the OS refuses to say.
        // Answering "not running" makes `status` settle a working attempt as
        // cut off and print `state: interrupted … Continue it with: tactus
        // resume <id>`, sending the operator to start a second engine on a
        // live run. Answering "running" costs a `status` that declines to
        // settle and says another process holds the run. One of those invents
        // a fact the operator will act on; the other admits the run may still
        // be going. `acquire` is the real guard against two engines either
        // way, and it reports this case as the IO error it is.
        Holder::Unknown(_) => true,
    }
}

#[cfg(unix)]
mod cleanup {
    use super::{cleanup_lock_file, refused};
    use crate::error::TactusError;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::marker::PhantomData;
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    thread_local! {
        // v0.1 drives a run synchronously inside an explicit scope. Thread-
        // local registration gives concurrent library/test runs the exact
        // cleanup path for their own reapers instead of conservatively leasing
        // every run active in the process.
        static ACTIVE: RefCell<BTreeMap<PathBuf, usize>> = const { RefCell::new(BTreeMap::new()) };
    }

    #[derive(Debug)]
    pub(super) struct CleanupLease {
        path: PathBuf,
    }

    #[derive(Debug)]
    pub(crate) struct CleanupScope<'a> {
        path: PathBuf,
        _lifetime_and_thread: PhantomData<(&'a CleanupLease, Rc<()>)>,
    }

    impl Drop for CleanupScope<'_> {
        fn drop(&mut self) {
            ACTIVE.with(|active| {
                let mut active = active.borrow_mut();
                let remove = if let Some(count) = active.get_mut(&self.path) {
                    *count = count.saturating_sub(1);
                    *count == 0
                } else {
                    false
                };
                if remove {
                    active.remove(&self.path);
                }
            });
        }
    }

    pub(super) fn enter(lease: &CleanupLease) -> CleanupScope<'_> {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            *active.entry(lease.path.clone()).or_default() += 1;
        });
        CleanupScope {
            path: lease.path.clone(),
            _lifetime_and_thread: PhantomData,
        }
    }

    pub(super) fn take(public: &Path) -> Result<CleanupLease, TactusError> {
        let path = cleanup_lock_file(public);
        let file = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| TactusError::Io {
                path: path.clone(),
                source,
            })?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let source = std::io::Error::last_os_error();
            if matches!(
                source.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            ) {
                return Err(refused(public, &path, None));
            }
            return Err(TactusError::Io { path, source });
        }
        // This probe proves no prior crash reaper remains. Do not retain the
        // lock in the conductor: arbitrary forked children would inherit its
        // open file description and recreate the false-liveness window the
        // primary fcntl lock deliberately avoids. Each cleanup reaper instead
        // reopens `path` and owns an independent shared hold.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } != 0 {
            return Err(TactusError::Io {
                path,
                source: std::io::Error::last_os_error(),
            });
        }
        Ok(CleanupLease { path })
    }

    pub(super) fn is_held(public: &Path) -> bool {
        let path = cleanup_lock_file(public);
        let file = match File::options().read(true).write(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            // An existing lease file that cannot be inspected is not evidence
            // that cleanup finished. Keep liveness fail-closed just as the
            // primary lock does for an unreportable holder.
            Err(_) => return true,
        };
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            false
        } else {
            true
        }
    }

    pub(crate) fn active_paths() -> Vec<PathBuf> {
        ACTIVE.with(|active| active.borrow().keys().cloned().collect())
    }
}

#[cfg(not(unix))]
mod cleanup {
    use crate::error::TactusError;
    use std::marker::PhantomData;
    use std::path::Path;
    use std::rc::Rc;

    #[derive(Debug)]
    pub(super) struct CleanupLease;

    #[derive(Debug)]
    pub(crate) struct CleanupScope<'a> {
        _lifetime_and_thread: PhantomData<(&'a CleanupLease, Rc<()>)>,
    }

    impl Drop for CleanupScope<'_> {
        fn drop(&mut self) {}
    }

    pub(super) fn take(_: &Path) -> Result<CleanupLease, TactusError> {
        Ok(CleanupLease)
    }

    pub(super) fn is_held(_: &Path) -> bool {
        false
    }

    pub(super) fn enter(_lease: &CleanupLease) -> CleanupScope<'_> {
        CleanupScope {
            _lifetime_and_thread: PhantomData,
        }
    }
}

#[cfg(unix)]
pub(crate) fn active_cleanup_lease_paths() -> Vec<PathBuf> {
    cleanup::active_paths()
}

/// The lock primitive, and why it is not `std`'s.
///
/// `File::try_lock` is `flock(2)` on Unix, and `flock` locks are held by the
/// *open file description*. `fork` duplicates every descriptor, so a child
/// inherits this lock and keeps holding it until it execs — which means an
/// engine that has finished and let go stays "locked" for as long as some
/// unrelated subprocess spawn is between `fork` and `exec`. Measured: hold a
/// lock, fork, release it, and a fresh probe still reports it taken.
///
/// That was papered over with a 500ms grace — believe contention only if it
/// persists — which is a timing proxy for a property the platform states
/// outright, and only ever probabilistic: a fork window longer than the grace
/// on a loaded machine still refuses a run that nothing is driving, and every
/// `status` of a live run paid the full half-second to find out.
///
/// `fcntl(F_SETLK)` locks are held by the *process*, and are documented not to
/// be inherited across `fork`. The grace disappears rather than being tuned.
///
/// Two things come with that, both measured rather than assumed:
///
/// - They do not exclude the same process from itself, which is what [`claims`]
///   is for.
/// - Closing **any** descriptor for the file releases every lock this process
///   holds on it, so a holder must never open its own lock file again.
///   [`is_running`] answers from [`claims`] before it would.
///
/// `F_OFD_SETLK` is not an escape from the first two: it is scoped to the open
/// file description, exactly like `flock`, and is inherited across `fork` in
/// exactly the same way.
///
/// Windows has neither hazard — `LockFileEx` is per-handle and there is no
/// `fork` — so it keeps std's implementation and this module is where the two
/// meet.
#[cfg(unix)]
mod imp {
    use super::Holder;
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;

    /// `F_WRLCK` and `F_UNLCK` are `c_int` on Linux and `c_short` on macOS,
    /// while `flock.l_type` is `c_short` on both. `Into<c_int>` accepts either
    /// — the reflexive conversion on Linux, the widening one on macOS — so the
    /// narrowing to `l_type` happens here and nowhere else.
    fn l_type(kind: impl Into<libc::c_int>) -> libc::c_short {
        kind.into() as libc::c_short
    }

    /// Take the exclusive lock, or say who has it.
    pub(super) fn take(file: &File) -> Holder {
        match set_lock(file, l_type(libc::F_WRLCK)) {
            Ok(()) => Holder::Nobody,
            Err(error) if would_block(&error) => Holder::Someone {
                pid: holding_pid(file),
            },
            Err(error) => Holder::Unknown(error),
        }
    }

    /// Ask who holds it, taking nothing. There is no shared lock to give back
    /// here and so no window in which this call is itself the holder.
    pub(super) fn holder(file: &File) -> Holder {
        match query(file) {
            Ok(Some(pid)) => Holder::Someone { pid: Some(pid) },
            Ok(None) => Holder::Nobody,
            Err(error) => Holder::Unknown(error),
        }
    }

    fn describe(kind: libc::c_short) -> libc::flock {
        libc::flock {
            l_type: kind,
            l_whence: libc::SEEK_SET as libc::c_short,
            // A zero length locks the whole file, however long it grows. The
            // file's contents are never read; it exists to be locked.
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        }
    }

    fn set_lock(file: &File, kind: libc::c_short) -> io::Result<()> {
        let request = describe(kind);
        // `F_SETLK` never blocks, so unlike `flock` it has no interruptible
        // wait to be cut short — the `EINTR` retry the old loop carried has
        // nothing left to guard.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &request) } == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }

    /// `Some(pid)` if a conflicting lock exists, `None` if the file is free.
    fn query(file: &File) -> io::Result<Option<u32>> {
        let mut request = describe(l_type(libc::F_WRLCK));
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut request) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if request.l_type == l_type(libc::F_UNLCK) {
            return Ok(None);
        }
        Ok(Some(u32::try_from(request.l_pid).unwrap_or_default()))
    }

    /// Best effort: the holder may let go between the refusal and the question,
    /// and a name that might be stale is worth more than no name at all.
    fn holding_pid(file: &File) -> Option<u32> {
        query(file).ok().flatten()
    }

    fn would_block(error: &io::Error) -> bool {
        // POSIX allows either, and says a portable caller must accept both.
        matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EACCES || code == libc::EAGAIN
        )
    }
}

#[cfg(not(unix))]
mod imp {
    use super::Holder;
    use std::fs::{File, TryLockError};

    pub(super) fn take(file: &File) -> Holder {
        match file.try_lock() {
            Ok(()) => Holder::Nobody,
            // `LockFileEx` names no owner, and inventing one would be worse
            // than the shorter sentence.
            Err(TryLockError::WouldBlock) => Holder::Someone { pid: None },
            Err(TryLockError::Error(source)) => Holder::Unknown(source),
        }
    }

    pub(super) fn holder(file: &File) -> Holder {
        match file.try_lock_shared() {
            Ok(()) => {
                let _ = file.unlock();
                Holder::Nobody
            }
            Err(TryLockError::WouldBlock) => Holder::Someone { pid: None },
            Err(TryLockError::Error(source)) => Holder::Unknown(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, Instant};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tactus-rundir-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn paths_in(root: &Path, run_id: &str) -> RunPaths {
        RunPaths::with_private_root(&root.join("repo"), run_id, &root.join("home"))
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
        assert_eq!(paths.events(), repo.join(".tactus/runs/RUN1/events.jsonl"));
    }

    #[test]
    fn the_private_fallback_is_never_the_workspace() {
        // No HOME is a bad day, not a reason to quietly put transcripts back
        // where an agent can read them.
        let root = default_private_root();
        assert!(
            root.ends_with(".tactus") || root.ends_with("tactus"),
            "{root:?}"
        );
        assert!(root.is_absolute(), "{root:?}");
    }

    #[test]
    fn runs_list_chronologically_and_resolve_by_prefix() {
        let root = scratch("discover");
        let repo = root.join("repo");
        for id in ["01AAA", "01BBB", "01BCC"] {
            fs::create_dir_all(runs_root(&repo).join(id)).expect("run dir");
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
            let dir = public_dir(&repo, run).join("questions");
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

    #[test]
    fn a_run_lock_remains_send_even_though_its_cleanup_scope_is_thread_local() {
        fn assert_send<T: Send>() {}
        assert_send::<RunLock>();
    }

    #[test]
    fn the_lock_answers_at_once_rather_than_waiting_to_be_sure() {
        // There was a 500ms contention grace here, and it was paid in full
        // exactly when the answer was yes: a live engine never lets go, so the
        // retry loop always ran to the deadline. Every `tactus status` and
        // `tactus answer` against a working run paid it, and `--follow` paid it
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
        let public = PathBuf::from(std::env::var("TACTUS_TEST_LOCK_DIR").expect("run dir"));
        let _held = RunLock::acquire(&public).expect("the child takes the lock");
        println!("held");
        std::io::Write::flush(&mut std::io::stdout()).expect("flush");
        std::thread::sleep(Duration::from_secs(30));
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
        let mut child = std::process::Command::new(exe)
            .args([
                "--exact",
                "rundir::tests::lock_child_holds_the_run",
                "--ignored",
                "--nocapture",
            ])
            .env("TACTUS_TEST_LOCK_DIR", &paths.public)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn the second engine");

        // Wait for it to say it has the lock, rather than sleeping and hoping.
        let mut out = std::io::BufReader::new(child.stdout.take().expect("stdout"));
        let mut line = String::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            line.clear();
            let read = std::io::BufRead::read_line(&mut out, &mut line).expect("read");
            assert!(read > 0, "the child ended without taking the lock");
            if line.trim() == "held" {
                break;
            }
            assert!(Instant::now() < deadline, "the child never took the lock");
        }

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
            err.to_string().contains(&format!("pid {}", child.id())),
            "the refusal should name the process actually holding it: {err}"
        );

        let _ = child.kill();
        let _ = child.wait();
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
        fs::create_dir_all(runs_root(&repo).join("01AbCd")).expect("run dir");

        assert_eq!(resolve_run_id(&repo, "01abcd").expect("exact"), "01AbCd");
        assert_eq!(resolve_run_id(&repo, "01AB").expect("prefix"), "01AbCd");
    }
}
