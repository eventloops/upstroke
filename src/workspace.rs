//! Workspace (DESIGN.md §6): the engine owns git. Agents edit files; only the
//! engine stages, commits, branches, and rolls back (invariant 1). Every git
//! operation is a subprocess of the system `git` binary — no library binding.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::TactusError;

pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Open an existing git worktree; refuses anything else.
    pub fn open(root: &Path) -> Result<Self, TactusError> {
        let ws = Self {
            root: root.to_path_buf(),
        };
        let inside = ws.git(&["rev-parse", "--is-inside-work-tree"])?;
        if inside.trim() != "true" {
            return Err(TactusError::Git {
                message: format!("{} is not a git worktree", root.display()),
            });
        }
        Ok(ws)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn git(&self, args: &[&str]) -> Result<String, TactusError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        if !output.status.success() {
            return Err(TactusError::Git {
                message: format!(
                    "git {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// §14 pre-flight: the engine refuses dirty trees.
    pub fn is_clean(&self) -> Result<bool, TactusError> {
        Ok(self.git(&["status", "--porcelain"])?.trim().is_empty())
    }

    pub fn current_branch(&self) -> Result<String, TactusError> {
        Ok(self
            .git(&["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_owned())
    }

    pub fn head_sha(&self) -> Result<String, TactusError> {
        Ok(self
            .git(&["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_owned())
    }

    pub fn create_branch(&self, name: &str) -> Result<(), TactusError> {
        self.git(&["switch", "-q", "-c", name]).map(|_| ())
    }

    /// Keep `.tactus/` (run dirs, transcripts) out of both `status` and the
    /// engine's own commits without touching the user's versioned .gitignore:
    /// append it to `.git/info/exclude`, which is local-only.
    pub fn ensure_run_exclusions(&self) -> Result<(), TactusError> {
        let git_dir = self.git(&["rev-parse", "--git-dir"])?;
        let git_dir = git_dir.trim();
        let mut exclude_path = PathBuf::from(git_dir);
        if exclude_path.is_relative() {
            exclude_path = self.root.join(exclude_path);
        }
        let exclude_path = exclude_path.join("info").join("exclude");
        let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == ".tactus/") {
            return Ok(());
        }
        if let Some(parent) = exclude_path.parent() {
            fs::create_dir_all(parent).map_err(|e| TactusError::Git {
                message: format!("creating {}: {e}", parent.display()),
            })?;
        }
        let mut updated = existing;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(".tactus/\n");
        fs::write(&exclude_path, updated).map_err(|e| TactusError::Git {
            message: format!("writing {}: {e}", exclude_path.display()),
        })
    }

    /// Stage everything and return the staged diff against HEAD — the
    /// engine-captured ground truth (invariant 3). Includes new files.
    pub fn capture_diff(&self) -> Result<String, TactusError> {
        self.git(&["add", "-A"])?;
        self.git(&["diff", "--cached"])
    }

    /// Commit whatever `capture_diff` staged. §14: commit-per-task,
    /// `[tactus] <task-id>: <title>`.
    pub fn commit(&self, message: &str) -> Result<String, TactusError> {
        self.git(&["commit", "-q", "-m", message])?;
        self.head_sha()
    }

    /// §14 rollback on failed attempt: back to the last commit, discarding
    /// staged, unstaged, and untracked changes.
    pub fn rollback(&self) -> Result<(), TactusError> {
        self.git(&["reset", "-q", "--hard", "HEAD"])?;
        self.git(&["clean", "-qfd"]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("tactus-ws-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create repo dir");
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@tactus.local"]);
        run(&["config", "user.name", "tactus tests"]);
        fs::write(dir.join("README.md"), "seed\n").expect("seed file");
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "seed"]);
        dir
    }

    #[test]
    fn open_requires_a_git_worktree() {
        let repo = temp_repo("open");
        assert!(Workspace::open(&repo).is_ok());

        let plain = env::temp_dir().join(format!("tactus-ws-plain-{}", std::process::id()));
        fs::create_dir_all(&plain).expect("plain dir");
        assert!(Workspace::open(&plain).is_err());
    }

    #[test]
    fn clean_detection_and_rollback() {
        let repo = temp_repo("clean");
        let ws = Workspace::open(&repo).expect("open");
        assert!(ws.is_clean().expect("clean check"));

        fs::write(repo.join("README.md"), "changed\n").expect("edit");
        fs::write(repo.join("stray.txt"), "untracked\n").expect("stray");
        assert!(!ws.is_clean().expect("dirty check"));

        ws.rollback().expect("rollback");
        assert!(ws.is_clean().expect("clean again"));
        assert!(!repo.join("stray.txt").exists(), "untracked cleaned");
        // core.autocrlf may legitimately restore CRLF on Windows checkouts.
        let readme = fs::read_to_string(repo.join("README.md")).expect("read");
        assert_eq!(readme.replace("\r\n", "\n"), "seed\n");
    }

    #[test]
    fn branch_diff_commit_cycle() {
        let repo = temp_repo("cycle");
        let ws = Workspace::open(&repo).expect("open");
        ws.create_branch("tactus/run-TEST").expect("branch");
        assert_eq!(ws.current_branch().expect("branch name"), "tactus/run-TEST");

        fs::write(repo.join("new.rs"), "fn main() {}\n").expect("new file");
        let diff = ws.capture_diff().expect("diff");
        assert!(diff.contains("new.rs"), "diff sees new files: {diff}");
        assert!(diff.contains("fn main"), "diff carries content");

        let sha = ws.commit("[tactus] t1: demo").expect("commit");
        assert!(!sha.is_empty());
        assert!(ws.is_clean().expect("clean after commit"));
        assert!(ws.capture_diff().expect("empty diff").trim().is_empty());
    }

    #[test]
    fn run_exclusions_hide_tactus_dir() {
        let repo = temp_repo("exclude");
        let ws = Workspace::open(&repo).expect("open");
        ws.ensure_run_exclusions().expect("exclude");
        ws.ensure_run_exclusions().expect("idempotent");
        fs::create_dir_all(repo.join(".tactus").join("runs")).expect("run dir");
        fs::write(repo.join(".tactus").join("runs").join("x.json"), "{}").expect("artifact");
        assert!(ws.is_clean().expect("tactus dir invisible"));
    }
}
