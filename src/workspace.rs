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
        let output = self.git_output(args)?;
        String::from_utf8(output).map_err(|error| TactusError::Git {
            message: format!(
                "git {} returned output that is not valid UTF-8: {error}",
                args.join(" ")
            ),
        })
    }

    fn git_output(&self, args: &[&str]) -> Result<Vec<u8>, TactusError> {
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
        Ok(output.stdout)
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

    /// Full HEAD sha. The event log records these rather than short ones
    /// because `--short` picks its length from `core.abbrev` and the repo's
    /// object count — a sha written by one checkout would not compare equal to
    /// the same sha read by another, which is exactly the check §15 asks
    /// `resume` to make.
    pub fn head_sha_full(&self) -> Result<String, TactusError> {
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_owned())
    }

    /// The full sha of a commit's first parent — `None` at a root commit.
    ///
    /// How `resume` tells a commit sitting directly on its own record apart
    /// from history that arrived some other way.
    pub fn parent_sha(&self, sha: &str) -> Result<Option<String>, TactusError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{sha}^"))
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        if !output.status.success() {
            // A root commit has no parent. That is an answer, not a failure.
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ))
    }

    /// A commit's subject — the first line of its message.
    pub fn commit_subject(&self, sha: &str) -> Result<String, TactusError> {
        Ok(self
            .git(&["log", "-1", "--format=%s", sha, "--"])?
            .trim()
            .to_owned())
    }

    pub fn create_branch(&self, name: &str) -> Result<(), TactusError> {
        self.git(&["switch", "-q", "-c", name]).map(|_| ())
    }

    /// Move to an existing branch — how `resume` gets back onto the run's own
    /// branch when the operator has wandered off it.
    pub fn switch_branch(&self, name: &str) -> Result<(), TactusError> {
        self.git(&["switch", "-q", name]).map(|_| ())
    }

    /// Whether a branch exists locally.
    pub fn branch_exists(&self, name: &str) -> Result<bool, TactusError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("refs/heads/{name}"))
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        Ok(output.status.success())
    }

    /// A one-line-per-path summary of everything uncommitted, for telling the
    /// operator what a resume is about to discard.
    pub fn uncommitted_summary(&self) -> Result<Vec<String>, TactusError> {
        Ok(self
            .git(&["status", "--porcelain"])?
            .lines()
            .map(|line| line.trim_end().to_owned())
            .filter(|line| !line.is_empty())
            .collect())
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
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
        ])
    }

    /// Refuse staged evidence whose bytes are not the bytes a gate would see,
    /// or whose worktree still contains unstaged nested state after `git add`.
    /// A clean/smudge filter makes the cached diff describe the transformed
    /// blob while gates see the smudged file. Dirty submodules similarly hide
    /// executable inputs behind an unchanged gitlink. Neither can be reviewed
    /// completely, so both are policy failures rather than gate results.
    pub fn review_input_problem(&self) -> Result<Option<String>, TactusError> {
        let status = self.git_output(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=no",
            "--ignore-submodules=none",
        ])?;
        for line in status
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            if line.len() < 3 || line[1] != b' ' {
                return Ok(Some(format!(
                    "the staged task still has unstaged or dirty nested-worktree state (`{}`); gates could observe bytes absent from the reviewed commit",
                    String::from_utf8_lossy(line)
                )));
            }
        }

        let names = self.git_output(&["diff", "--cached", "--name-only", "-z"])?;
        for raw in names
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let path = std::str::from_utf8(raw).map_err(|_| TactusError::Git {
                message: "a staged path is not valid UTF-8, so its attributes cannot be verified"
                    .to_owned(),
            })?;
            let attr = self.git_output(&["check-attr", "--cached", "-z", "filter", "--", path])?;
            let fields: Vec<&[u8]> = attr
                .split(|byte| *byte == 0)
                .filter(|field| !field.is_empty())
                .collect();
            let value = fields.get(2).copied().unwrap_or_default();
            if !matches!(value, b"unspecified" | b"unset") {
                return Ok(Some(format!(
                    "staged path `{path}` uses clean/smudge filter `{}`; the cached diff and gate worktree can contain different bytes",
                    String::from_utf8_lossy(value)
                )));
            }
        }
        Ok(None)
    }

    /// A clean detached worktree whose HEAD tree is exactly the staged tree.
    /// Gates run here, never in the worker's workspace, so ignored files,
    /// build residue, and gate side-effects cannot influence or contaminate the
    /// commit under review.
    pub fn gate_snapshot(&self) -> Result<GateWorkspace, TactusError> {
        let tree = self.git(&["write-tree"])?;
        let commit = self.git(&[
            "-c",
            "user.name=tactus",
            "-c",
            "user.email=tactus@tactus.local",
            "commit-tree",
            tree.trim(),
            "-p",
            "HEAD",
            "-m",
            "[tactus] ephemeral gate snapshot",
        ])?;
        let path = std::env::temp_dir().join(format!(
            "tactus-gates-{}-{}",
            std::process::id(),
            crate::ulid::ulid()
        ));
        let path_text = path.to_str().ok_or_else(|| TactusError::Git {
            message: format!("gate snapshot path is not valid UTF-8: {}", path.display()),
        })?;
        self.git(&[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--force",
            path_text,
            commit.trim(),
        ])?;
        match Workspace::open(&path) {
            Ok(workspace) => Ok(GateWorkspace {
                source_root: self.root.clone(),
                path,
                workspace,
            }),
            Err(error) => {
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&self.root)
                    .args(["worktree", "remove", "--force"])
                    .arg(&path)
                    .output();
                Err(error)
            }
        }
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

pub struct GateWorkspace {
    source_root: PathBuf,
    path: PathBuf,
    workspace: Workspace,
}

impl GateWorkspace {
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
}

impl Drop for GateWorkspace {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.source_root)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
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

        // What `resume` reads to recognise a commit as its own.
        let full = ws.head_sha_full().expect("full sha");
        assert_eq!(
            ws.commit_subject(&full).expect("subject"),
            "[tactus] t1: demo"
        );
        let parent = ws.parent_sha(&full).expect("parent").expect("has a parent");
        assert_ne!(parent, full);
        assert_eq!(
            ws.parent_sha(&parent).expect("root lookup"),
            None,
            "the seed commit is the root, and that is an answer rather than an error"
        );
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

    #[test]
    fn opaque_git_diffs_are_rejected_before_review() {
        let repo = temp_repo("opaque-diff");
        let ws = Workspace::open(&repo).expect("open");

        // A candidate controls .gitattributes. Without --binary, marking a
        // source path -diff replaces all changed bytes with the tiny sentence
        // "Binary files differ", which a read-only reviewer cannot recover.
        fs::write(repo.join(".gitattributes"), "hidden.rs -diff\n").expect("attributes");
        fs::write(repo.join("hidden.rs"), "fn hidden_change() {}\n").expect("hidden source");
        fs::write(repo.join("asset.bin"), b"\0opaque bytes\xff").expect("binary asset");

        let diff = ws.capture_diff().expect("binary-complete diff");
        assert!(
            diff.lines().any(|line| line == "GIT binary patch"),
            "opaque paths must be represented explicitly: {diff}"
        );
        let refusal = crate::review::complete_diff_error(&diff)
            .expect("an opaque patch cannot receive a semantic review");
        assert!(refusal.to_string().contains("opaque binary"), "{refusal}");
    }

    #[test]
    fn non_utf8_text_diff_is_refused_before_review() {
        let repo = temp_repo("non-utf8-diff");
        let ws = Workspace::open(&repo).expect("open");
        fs::write(repo.join("invalid.rs"), b"fn changed() { // \xff\n}\n").expect("invalid text");

        let error = ws
            .capture_diff()
            .expect_err("lossy conversion would change the evidence the reviewer sees");
        assert!(error.to_string().contains("not valid UTF-8"), "{error}");
    }

    #[test]
    fn ignored_worker_input_is_absent_from_gate_snapshot() {
        let repo = temp_repo("ignored-gate-input");
        let ws = Workspace::open(&repo).expect("open");
        fs::write(repo.join(".gitignore"), "worker-toggle\n").expect("ignore rule");
        ws.capture_diff().expect("stage ignore rule");
        ws.commit("seed ignore rule").expect("commit ignore rule");

        fs::write(repo.join("README.md"), "changed\n").expect("tracked edit");
        fs::write(repo.join("worker-toggle"), "make the gate pass\n").expect("ignored input");
        ws.capture_diff().expect("stage candidate");
        assert!(
            ws.review_input_problem()
                .expect("inspect evidence")
                .is_none(),
            "ignored state is isolated by materialization rather than misreported as staged"
        );

        let snapshot = ws.gate_snapshot().expect("exact staged snapshot");
        assert_eq!(
            fs::read_to_string(snapshot.workspace().root().join("README.md"))
                .expect("snapshot tracked file")
                .replace("\r\n", "\n"),
            "changed\n"
        );
        assert!(
            !snapshot.workspace().root().join("worker-toggle").exists(),
            "worker-created ignored input must not reach gates"
        );
        assert!(snapshot.workspace().is_clean().expect("clean snapshot"));
    }

    #[test]
    fn filtered_paths_are_refused_before_gates_and_review() {
        let repo = temp_repo("filtered-evidence");
        let ws = Workspace::open(&repo).expect("open");
        fs::write(
            repo.join(".gitattributes"),
            "filtered.txt filter=tactus-test\n",
        )
        .expect("filter attribute");
        fs::write(repo.join("filtered.txt"), "semantic bytes\n").expect("filtered file");
        ws.capture_diff().expect("stage filtered path");

        let problem = ws
            .review_input_problem()
            .expect("inspect attributes")
            .expect("filtered evidence must fail closed");
        assert!(problem.contains("filtered.txt"), "{problem}");
        assert!(problem.contains("tactus-test"), "{problem}");
        assert!(problem.contains("different bytes"), "{problem}");
    }

    #[test]
    fn dirty_submodule_worktree_is_refused_before_gates() {
        let child = temp_repo("dirty-submodule-child");
        let repo = temp_repo("dirty-submodule-parent");
        let add = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "protocol.file.allow=always", "submodule", "add", "-q"])
            .arg(&child)
            .arg("nested")
            .output()
            .expect("add submodule");
        assert!(
            add.status.success(),
            "submodule add: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let ws = Workspace::open(&repo).expect("open parent");
        ws.capture_diff().expect("stage gitlink");
        ws.commit("seed submodule").expect("commit gitlink");
        fs::write(
            repo.join("nested").join("README.md"),
            "dirty nested bytes\n",
        )
        .expect("dirty submodule");
        ws.capture_diff().expect("stage parent");

        let problem = ws
            .review_input_problem()
            .expect("inspect nested state")
            .expect("dirty submodule must fail closed");
        assert!(problem.contains("nested"), "{problem}");
        assert!(
            problem.contains("absent from the reviewed commit"),
            "{problem}"
        );
    }
}
