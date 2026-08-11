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

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// An exclusive hold on one run, released when this value drops.
///
/// Two engines on one run directory would interleave events into the log and
/// fight over the same git branch and working tree. An advisory OS lock is the
/// right shape for that because the operating system releases it when the
/// holder dies: a crashed run — the case `resume` exists for — leaves no stale
/// marker to clear by hand, which is exactly what a lock *file* would have
/// forced on the common path.
#[derive(Debug)]
pub struct RunLock {
    _file: File,
}

impl RunLock {
    /// Take the lock on a run's public directory, or explain who has it.
    pub fn acquire(public: &Path) -> Result<Self, TactusError> {
        let path = lock_file(public);
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| TactusError::Io {
                path: path.clone(),
                source,
            })?;
        // Only *contention* may be reported as contention, and two things can
        // fake it.
        //
        // `flock` is interruptible: a signal — SIGCHLD from any of the git and
        // agent subprocesses a run spawns — makes `try_lock` fail with
        // `Interrupted`, having learned nothing about who holds what.
        //
        // And `flock` is inherited across `fork`. A subprocess spawned by
        // *anything else in this process* duplicates every open descriptor,
        // this lock among them, and the child keeps holding it until it execs
        // (where close-on-exec drops it). So an uncontested lock reads as taken
        // for as long as some unrelated fork is between `fork` and `exec`.
        // Measured under a parallel test suite, where it made runs refuse to
        // start against a second engine that did not exist.
        //
        // What separates the two from the real thing is duration: a genuine
        // engine holds the lock for its whole run, a fork window for
        // microseconds. So believe `WouldBlock` only if it persists.
        match probe(|| file.try_lock(), CONTENTION_GRACE) {
            Verdict::Free => Ok(Self { _file: file }),
            Verdict::Held => Err(TactusError::Refused {
                message: format!(
                    "another tactus process is already driving run `{}` (lock held on {}). Two \
                     engines would interleave events and fight over the same branch — wait for it \
                     to finish, or stop it first.",
                    public.file_name().unwrap_or_default().to_string_lossy(),
                    path.display()
                ),
            }),
            // A lock that cannot be taken is not a lock that was taken. Say
            // what actually failed rather than blaming an engine that may not
            // exist.
            Verdict::Unanswered(source) => Err(TactusError::Io { path, source }),
        }
    }
}

/// What one lock call, retried until it means something, established.
#[derive(Debug)]
enum Verdict {
    /// Nobody holds it.
    Free,
    /// Somebody does, and went on holding it long enough to be believed.
    Held,
    /// The call failed without answering the question.
    Unanswered(io::Error),
}

/// Retry a lock call until it says something a caller can act on.
///
/// Both callers below face the same problem — a single `try_lock` reports
/// contention that is not contention — and both want the same discipline
/// applied to it, so they share one loop rather than two that have to be kept
/// in step. `grace` is a parameter because it is the only thing they disagree
/// about: a caller that will ask again in half a second wants the answer now.
fn probe(mut attempt: impl FnMut() -> Result<(), TryLockError>, grace: Duration) -> Verdict {
    let deadline = Instant::now() + grace;
    loop {
        match attempt() {
            Ok(()) => return Verdict::Free,
            Err(TryLockError::Error(source)) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(TryLockError::Error(source)) => return Verdict::Unanswered(source),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Verdict::Held;
                }
                std::thread::sleep(RETRY_PAUSE);
            }
        }
    }
}

/// How long an apparently-held lock must stay held before it is believed.
///
/// Long enough to outlast any `fork`/`exec` window, short enough that a real
/// second engine still refuses promptly rather than appearing to hang.
const CONTENTION_GRACE: Duration = Duration::from_millis(500);
const RETRY_PAUSE: Duration = Duration::from_millis(5);

/// Whether a run is being driven right now, without disturbing the holder.
///
/// Read-only: `status` uses it to tell a live run from an interrupted one, so
/// it takes a shared lock and immediately gives it back. A file that cannot be
/// opened at all is not a running run — the lock is only ever created by a run
/// that started.
pub fn is_running(public: &Path) -> bool {
    let Ok(file) = File::open(lock_file(public)) else {
        return false;
    };
    // The same two traps `RunLock::acquire` documents, and the same answer: a
    // `fork` anywhere else in this process duplicates the descriptor so that an
    // *unheld* lock refuses a shared one until that child execs, and that must
    // not read as "a run is live" — which is the one thing `status` must not
    // invent, since it is what tells an operator their run is still going.
    match probe(|| file.try_lock_shared(), CONTENTION_GRACE) {
        Verdict::Free => {
            let _ = file.unlock();
            false
        }
        Verdict::Held => true,
        // The opened-fine-but-cannot-be-locked case, which is not the same as
        // the unopenable file above and does not get the same answer.
        // `flock` returns `ENOLCK` or `EOPNOTSUPP` on filesystems that do not
        // carry locks — NFS, SMB, some container overlays — and it does so
        // whether or not an engine is driving the run.
        //
        // So the question is which way to be wrong when the OS refuses to say.
        // Answering "not running" makes `status` settle a working attempt as
        // cut off and print `state: interrupted … Continue it with: tactus
        // resume <id>`, sending the operator to start a second engine on a
        // live run. Answering "running" costs a `status` that declines to
        // settle and says another process holds the run. One of those invents
        // a fact the operator will act on; the other admits the run may still
        // be going. `acquire` is the real guard against two engines either
        // way, and it now reports this case as the IO error it is.
        Verdict::Unanswered(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Timed, because the refusal now waits before it believes itself: a
        // `fork` elsewhere in the process can make an uncontested lock look
        // taken for a moment, so `acquire` re-checks for `CONTENTION_GRACE`
        // before reporting contention. That grace must stay short enough to
        // read as a refusal rather than a hang — raise it much further and the
        // operator is left watching a command that looks stuck.
        let started = Instant::now();
        let err = RunLock::acquire(&paths.public).expect_err("second engine must be refused");
        let waited = started.elapsed();
        assert!(
            err.to_string().contains("already driving run"),
            "got: {err}"
        );
        assert!(
            waited >= CONTENTION_GRACE,
            "a real holder is only believed after the grace: {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(5),
            "and refusing must still feel like a refusal: {waited:?}"
        );

        // Dropping releases it — which is also what a crash does, so resume
        // never has to clear a stale marker by hand.
        drop(held);
        assert!(!is_running(&paths.public));
        RunLock::acquire(&paths.public).expect("re-acquire after release");
    }

    #[test]
    fn a_lock_held_for_only_an_instant_is_not_a_running_run() {
        // The reading half of what `a_run_can_only_be_held_once_at_a_time`
        // times on `acquire`. That test holds the lock permanently and drops
        // it permanently, so `is_running` could go back to believing every
        // refusal on sight and stay green — which is exactly the state it was
        // in when a `fork` window made a finished run report itself as live.
        //
        // The cost was not only a wrong status line. `RunReport` carries
        // `running`, and it decides whether unreached tasks settle as skipped
        // or stay pending, so one spurious yes made a run's own report differ
        // from a replay of its log — the invariant the event log rests on.
        // Measured at 50 false positives in 3000 probes under a parallel suite
        // spawning subprocesses.
        let root = scratch("transient");
        let paths = paths_in(&root, "RUN1");
        paths.create().expect("create");

        let held = RunLock::acquire(&paths.public).expect("acquire");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(CONTENTION_GRACE / 5);
            drop(held);
        });
        assert!(
            !is_running(&paths.public),
            "a hold that does not outlast the grace is a fork window, not an engine"
        );
        releaser.join().expect("releaser");
    }

    #[test]
    fn a_lock_call_that_never_answers_is_not_a_free_lock() {
        // No filesystem CI runs on returns `ENOLCK`, so the decision is tested
        // where it is made rather than through a real lock. The arms that
        // matter are the two that are not `Ok`: a lock the OS declines to
        // report on must not come back as "nobody is running", because that is
        // the reading that tells an operator to resume a run still in flight.
        let unsupported = || {
            Err(TryLockError::Error(io::Error::from_raw_os_error(
                ENOLCK_LIKE,
            )))
        };
        assert!(
            matches!(probe(unsupported, Duration::ZERO), Verdict::Unanswered(_)),
            "an error is not an answer"
        );
        assert!(matches!(probe(|| Ok(()), Duration::ZERO), Verdict::Free));
        assert!(matches!(
            probe(|| Err(TryLockError::WouldBlock), Duration::ZERO),
            Verdict::Held
        ));
    }

    /// Any errno that is not `Interrupted`; the value itself does not matter.
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
