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
    /// Open an existing git worktree, normalizing to its top level. Running
    /// from a subdirectory would otherwise scope `git clean` to that
    /// subdirectory while staging stays whole-tree, so rollback would leave
    /// residue above the current directory.
    pub fn open(root: &Path) -> Result<Self, TactusError> {
        let probe = Self {
            root: root.to_path_buf(),
        };
        let inside = probe.git(&["rev-parse", "--is-inside-work-tree"])?;
        if inside.trim() != "true" {
            return Err(TactusError::Git {
                message: format!("{} is not a git worktree", root.display()),
            });
        }
        let toplevel = probe.git(&["rev-parse", "--show-toplevel"])?;
        let toplevel = toplevel.trim();
        Ok(Self {
            root: if toplevel.is_empty() {
                probe.root
            } else {
                PathBuf::from(toplevel)
            },
        })
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

    /// Keep `.tactus/` (run dirs, transcripts) out of `status` and out of the
    /// engine's own commits.
    ///
    /// This is a self-ignoring `.tactus/.gitignore` containing `*` (the
    /// pattern cargo uses for `target/`) rather than an entry in
    /// `.git/info/exclude`: it needs no read-modify-write of a file the user
    /// owns, disappears with the directory, and — unlike `info/exclude` under
    /// `--git-dir` — behaves correctly in a linked worktree, where git reads
    /// excludes only from the common directory.
    pub fn ensure_run_exclusions(&self) -> Result<(), TactusError> {
        let dir = self.root.join(".tactus");
        fs::create_dir_all(&dir).map_err(|e| TactusError::Git {
            message: format!("creating {}: {e}", dir.display()),
        })?;
        let ignore_path = dir.join(".gitignore");
        if fs::read_to_string(&ignore_path).is_ok_and(|c| c.contains('*')) {
            return Ok(());
        }
        fs::write(&ignore_path, "*\n").map_err(|e| TactusError::Git {
            message: format!("writing {}: {e}", ignore_path.display()),
        })
    }

    /// Stage everything and return the staged diff against HEAD — the
    /// engine-captured ground truth (invariant 3). Includes new files.
    ///
    /// The diff must be a plain unified diff regardless of user config: a
    /// configured `diff.external` (difftastic and friends) would replace it
    /// wholesale and `color.ui` would inject escape codes, corrupting every
    /// downstream check that reads it.
    pub fn capture_diff(&self) -> Result<String, TactusError> {
        self.git(&["add", "-A"])?;
        self.git(&[
            "-c",
            "color.ui=false",
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-color",
        ])
    }

    /// Commit whatever `capture_diff` staged. §14: commit-per-task,
    /// `[tactus] <task-id>: <title>`.
    pub fn commit(&self, message: &str) -> Result<String, TactusError> {
        self.git(&["commit", "-q", "-m", message])?;
        self.head_sha()
    }

    /// Discard everything since the last commit: staged, unstaged, and
    /// untracked (ignored files survive). This is both the §14 rollback on a
    /// failed attempt and the post-commit scrub that keeps gate side-effects
    /// (build artifacts, lockfile churn) from leaking into the next task's
    /// captured diff.
    pub fn discard_uncommitted(&self) -> Result<(), TactusError> {
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

        ws.discard_uncommitted().expect("discard");
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
        assert!(
            ws.capture_diff().expect("diff").trim().is_empty(),
            "run artifacts never enter a commit"
        );
    }

    #[test]
    fn exclusions_work_in_a_linked_worktree() {
        let repo = temp_repo("worktree-main");
        let linked = repo
            .parent()
            .expect("parent")
            .join(format!("tactus-ws-worktree-linked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&linked);
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["worktree", "add", "-q", "-b", "wt"])
            .arg(&linked)
            .output()
            .expect("git worktree add");
        assert!(
            out.status.success(),
            "worktree add: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let ws = Workspace::open(&linked).expect("open linked worktree");
        ws.ensure_run_exclusions().expect("exclude");
        fs::create_dir_all(linked.join(".tactus").join("runs")).expect("run dir");
        fs::write(linked.join(".tactus").join("runs").join("t.json"), "{}").expect("artifact");
        assert!(
            ws.is_clean().expect("status"),
            "linked worktrees read excludes from the common dir, so info/exclude would not work"
        );
    }

    #[test]
    fn open_normalizes_to_the_worktree_toplevel() {
        let repo = temp_repo("toplevel");
        let nested = repo.join("crates").join("inner");
        fs::create_dir_all(&nested).expect("nested dirs");
        let ws = Workspace::open(&nested).expect("open from a subdirectory");
        // Compare canonically: temp dirs may be reached via a symlinked path.
        let expected = fs::canonicalize(&repo).expect("canonical repo");
        let actual = fs::canonicalize(ws.root()).expect("canonical root");
        assert_eq!(
            actual, expected,
            "root normalized to the worktree top level"
        );
    }

    #[test]
    fn capture_diff_is_immune_to_user_diff_config() {
        let repo = temp_repo("extdiff");
        // Simulate a user with difftastic-style config and forced color.
        let set = |k: &str, v: &str| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["config", "--local", k, v])
                .output()
                .expect("git config");
            assert!(out.status.success());
        };
        set("diff.external", "definitely-not-a-real-differ");
        set("color.ui", "always");
        set("color.diff", "always");

        let ws = Workspace::open(&repo).expect("open");
        fs::write(repo.join("new.rs"), "#[test]\nfn works() {}\n").expect("file");
        let diff = ws.capture_diff().expect("diff");
        assert!(diff.contains("+++ "), "plain unified diff: {diff}");
        assert!(!diff.contains('\u{1b}'), "no ANSI escapes: {diff}");
    }
}
