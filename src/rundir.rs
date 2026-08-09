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

use std::fs::{self, File};
use std::path::{Path, PathBuf};

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
        self.public.join("run.lock")
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
    if runs.iter().any(|id| id.eq_ignore_ascii_case(wanted)) {
        return Ok(wanted_upper);
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
    /// Take the lock, or explain who has it.
    pub fn acquire(paths: &RunPaths) -> Result<Self, TactusError> {
        let path = paths.lock_file();
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| TactusError::Io {
                path: path.clone(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(_) => Err(TactusError::Refused {
                message: format!(
                    "another tactus process is already driving run `{}` (lock held on {}). Two \
                     engines would interleave events and fight over the same branch — wait for \
                     it to finish, or stop it first.",
                    paths
                        .public
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    path.display()
                ),
            }),
        }
    }
}

/// Whether a run is being driven right now, without disturbing the holder.
///
/// Read-only: `status` uses it to tell a live run from an interrupted one, so
/// it takes a shared lock and immediately gives it back. A file that cannot be
/// opened at all is not a running run — the lock is only ever created by a run
/// that started.
pub fn is_running(paths: &RunPaths) -> bool {
    let Ok(file) = File::open(paths.lock_file()) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(_) => true,
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
            !is_running(&paths),
            "nothing holds a run that never started"
        );
        let held = RunLock::acquire(&paths).expect("first acquire");
        assert!(is_running(&paths), "status can see the run is live");

        let err = RunLock::acquire(&paths).expect_err("second engine must be refused");
        assert!(
            err.to_string().contains("already driving run"),
            "got: {err}"
        );

        // Dropping releases it — which is also what a crash does, so resume
        // never has to clear a stale marker by hand.
        drop(held);
        assert!(!is_running(&paths));
        RunLock::acquire(&paths).expect("re-acquire after release");
    }
}
