//! Workspace (DESIGN.md §6): the engine owns git. Agents edit files; only the
//! engine stages, commits, branches, and rolls back (invariant 1). Every git
//! operation is a subprocess of the system `git` binary — no library binding.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::error::TactusError;
use crate::events::PreparedCommit;

pub struct Workspace {
    root: PathBuf,
}

/// The immutable candidate captured immediately after staging. Every gate,
/// review, prepared commit, and CAS uses these object identities rather than
/// consulting a mutable index again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedCandidate {
    pub parent_oid: String,
    pub tree_oid: String,
    pub diff: String,
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

    /// Run a Git command with every repository-configured hook and fsmonitor
    /// disabled. Keep this raw-output primitive reusable by reference updates,
    /// whose expected compare-and-swap failures need the real exit status.
    pub(crate) fn run_git_with_private_hooks(&self, args: &[&str]) -> Result<Output, TactusError> {
        let hooks = PrivateHooksDir::create()?;
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(&hooks.path);
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .arg("-c")
            .arg(hooks_config)
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })
    }

    fn git_output_with_private_hooks(&self, args: &[&str]) -> Result<Vec<u8>, TactusError> {
        let output = self.run_git_with_private_hooks(args)?;
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

    fn git_with_private_hooks(&self, args: &[&str]) -> Result<String, TactusError> {
        let output = self.git_output_with_private_hooks(args)?;
        String::from_utf8(output).map_err(|error| TactusError::Git {
            message: format!(
                "git {} returned output that is not valid UTF-8: {error}",
                args.join(" ")
            ),
        })
    }

    fn git_output_with_input(&self, args: &[&str], input: Vec<u8>) -> Result<Vec<u8>, TactusError> {
        let hooks = PrivateHooksDir::create()?;
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(&hooks.path);
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .arg("-c")
            .arg(hooks_config)
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| TactusError::Git {
            message: format!("git {} did not open stdin", args.join(" ")),
        })?;
        // Read stdout/stderr while feeding the complete NUL-delimited path
        // list. A large index can otherwise fill check-attr's stdout pipe and
        // deadlock the parent while it is still writing stdin.
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output().map_err(|e| TactusError::Git {
            message: format!("waiting for git {}: {e}", args.join(" ")),
        })?;
        let write_result = writer.join().map_err(|_| TactusError::Git {
            message: format!("writing paths to git {} panicked", args.join(" ")),
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
        write_result.map_err(|e| TactusError::Git {
            message: format!("writing paths to git {}: {e}", args.join(" ")),
        })?;
        Ok(output.stdout)
    }

    fn prepared_update_ref(&self, args: &[&str]) -> Result<(), TactusError> {
        let output = self.run_git_with_private_hooks(args)?;
        if !output.status.success() {
            return Err(TactusError::Git {
                message: format!(
                    "git {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(())
    }

    fn commit_tree_with_tactus_identity(
        &self,
        tree_oid: &str,
        parent_oid: &str,
        message: &str,
    ) -> Result<String, TactusError> {
        let args = ["commit-tree", tree_oid, "-p", parent_oid, "-m", message];
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            // Environment identity overrides repository/global config and any
            // inherited GIT_AUTHOR_* or GIT_COMMITTER_* values.
            .env("GIT_AUTHOR_NAME", "tactus")
            .env("GIT_AUTHOR_EMAIL", "tactus@tactus.local")
            .env("GIT_COMMITTER_NAME", "tactus")
            .env("GIT_COMMITTER_EMAIL", "tactus@tactus.local")
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
        String::from_utf8(output.stdout).map_err(|error| TactusError::Git {
            message: format!(
                "git {} returned output that is not valid UTF-8: {error}",
                args.join(" ")
            ),
        })
    }

    /// §14 pre-flight: the engine refuses dirty trees.
    pub fn is_clean(&self) -> Result<bool, TactusError> {
        self.refuse_worktree_filters_before("git status")?;
        Ok(self
            .git_with_private_hooks(&["status", "--porcelain"])?
            .trim()
            .is_empty())
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
        self.refuse_worktree_filters_before("git switch")?;
        let tree_oid = self.git(&["rev-parse", "HEAD^{tree}"])?;
        self.refuse_unsafe_checkout_tree(tree_oid.trim())?;
        let create = format!("--create={name}");
        self.git_with_private_hooks(&["switch", "-q", "--no-recurse-submodules", &create, "--"])
            .map(|_| ())
    }

    /// Move to an existing branch — how `resume` gets back onto the run's own
    /// branch when the operator has wandered off it.
    pub fn switch_branch(&self, name: &str) -> Result<(), TactusError> {
        self.refuse_worktree_filters_before("git switch")?;
        let revision = format!("refs/heads/{name}^{{tree}}");
        let tree_oid = self.git(&["rev-parse", "--verify", &revision])?;
        self.refuse_unsafe_checkout_tree(tree_oid.trim())?;
        self.git_with_private_hooks(&["switch", "-q", "--no-recurse-submodules", "--", name])
            .map(|_| ())
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
        self.refuse_worktree_filters_before("git status")?;
        Ok(self
            .git_with_private_hooks(&["status", "--porcelain"])?
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

    /// Stage everything, freeze one parent and tree object, and return their
    /// complete diff. The diff names those frozen objects rather than rereading
    /// HEAD or the index, so all three values remain one candidate even if a
    /// ref or the index changes afterward.
    ///
    /// The diff must be a plain unified diff regardless of user config: a
    /// configured `diff.external` (difftastic and friends) would replace it
    /// wholesale and `color.ui` would inject escape codes, corrupting every
    /// downstream check that reads it.
    pub fn capture_candidate(&self) -> Result<CapturedCandidate, TactusError> {
        let parent_oid = self.head_sha_full()?;
        if let Some(problem) = self.worktree_filter_problem("git add")? {
            return Err(TactusError::Git { message: problem });
        }
        self.git_with_private_hooks(&["add", "-A"])?;
        let tree_oid = self.staged_tree_oid()?;
        let diff = self.git(&[
            "-c",
            "color.ui=false",
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            &parent_oid,
            &tree_oid,
            "--",
        ])?;
        let observed_parent = self.head_sha_full()?;
        if observed_parent != parent_oid {
            return Err(TactusError::Git {
                message: format!(
                    "HEAD moved from {parent_oid} to {observed_parent} while capturing the candidate"
                ),
            });
        }
        Ok(CapturedCandidate {
            parent_oid,
            tree_oid,
            diff,
        })
    }

    /// Backward-compatible diff-only capture for existing callers.
    pub fn capture_diff(&self) -> Result<String, TactusError> {
        Ok(self.capture_candidate()?.diff)
    }

    fn worktree_filter_problem(&self, operation: &str) -> Result<Option<String>, TactusError> {
        // Commands that inspect or update worktree entries (`add`, `status`,
        // `switch`, `commit`) can run clean/process filters before a later
        // tree policy check. Enumerate tracked and addable untracked paths
        // without refreshing fsmonitor, then evaluate the worktree's
        // attributes without invoking a driver.
        let paths = self.git_output_with_private_hooks(&[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])?;
        self.filter_problem_for_paths(paths, None, operation)
    }

    fn refuse_worktree_filters_before(&self, operation: &str) -> Result<(), TactusError> {
        if let Some(problem) = self.worktree_filter_problem(operation)? {
            return Err(TactusError::Git { message: problem });
        }
        Ok(())
    }

    /// Refuse staged evidence whose bytes are not the bytes a gate would see,
    /// or whose worktree still contains unstaged nested state after `git add`.
    /// A clean/smudge filter makes the cached diff describe the transformed
    /// blob while gates see the smudged file. Dirty submodules similarly hide
    /// executable inputs behind an unchanged gitlink. Neither can be reviewed
    /// completely, so both are policy failures rather than gate results.
    pub fn review_input_problem(&self) -> Result<Option<String>, TactusError> {
        let tree_oid = self.staged_tree_oid()?;
        self.review_input_problem_for_tree(&tree_oid)
    }

    /// Inspect live nested-worktree state, then bind every semantic input check
    /// to one captured tree rather than to an index that may have moved since
    /// its diff was produced.
    pub fn review_input_problem_for_tree(
        &self,
        tree_oid: &str,
    ) -> Result<Option<String>, TactusError> {
        self.validate_tree_oid(tree_oid)?;
        if let Some(problem) = self.worktree_filter_problem("git status")? {
            return Ok(Some(problem));
        }
        let status = self.git_output_with_private_hooks(&[
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

        self.tree_input_problem(tree_oid)
    }

    fn tree_input_problem(&self, tree_oid: &str) -> Result<Option<String>, TactusError> {
        // A captured .gitattributes can attach a filter to an otherwise
        // unchanged file, so changed names are insufficient. `ls-tree`
        // enumerates every path in the exact candidate and exposes gitlinks.
        let entries = self.git_output(&["ls-tree", "-r", "-z", "--full-tree", tree_oid])?;
        let mut paths = Vec::new();
        for entry in entries
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let tab = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| TactusError::Git {
                    message: "git ls-tree returned a malformed tree entry".to_owned(),
                })?;
            let metadata = &entry[..tab];
            let path = &entry[tab + 1..];
            let mode = metadata
                .split(|byte| *byte == b' ')
                .next()
                .unwrap_or_default();
            if mode == b"160000" {
                return Ok(Some(format!(
                    "candidate-tree path `{}` is a submodule (mode 160000); exact gate snapshots do not materialize submodules",
                    String::from_utf8_lossy(path)
                )));
            }
            paths.extend_from_slice(path);
            paths.push(0);
        }

        self.filter_problem_for_paths(paths, Some(tree_oid), "")
    }

    fn filter_problem_for_paths(
        &self,
        paths: Vec<u8>,
        tree_oid: Option<&str>,
        operation: &str,
    ) -> Result<Option<String>, TactusError> {
        let attrs = if paths.is_empty() {
            Vec::new()
        } else if let Some(tree_oid) = tree_oid {
            let source = format!("--source={tree_oid}");
            self.git_output_with_input(&["check-attr", &source, "--stdin", "-z", "filter"], paths)?
        } else {
            self.git_output_with_input(&["check-attr", "--stdin", "-z", "filter"], paths)?
        };
        let mut fields: Vec<&[u8]> = attrs.split(|byte| *byte == 0).collect();
        if fields.last().is_some_and(|field| field.is_empty()) {
            fields.pop();
        }
        if !fields.len().is_multiple_of(3) {
            return Err(TactusError::Git {
                message: "git check-attr returned malformed NUL-delimited output".to_owned(),
            });
        }
        for record in fields.chunks_exact(3) {
            let path = record[0];
            let attribute = record[1];
            let value = record[2];
            if attribute != b"filter" {
                return Err(TactusError::Git {
                    message: "git check-attr returned an unexpected attribute".to_owned(),
                });
            }
            if !matches!(value, b"unspecified" | b"unset") {
                let path = String::from_utf8_lossy(path);
                let value = String::from_utf8_lossy(value);
                return Ok(Some(if tree_oid.is_some() {
                    format!(
                        "candidate-tree path `{path}` uses clean/smudge filter `{value}`; the captured diff and gate worktree can contain different bytes"
                    )
                } else {
                    format!(
                        "working-tree path `{path}` uses clean/smudge filter `{value}`; refusing before {operation} can execute configured filter code"
                    )
                }));
            }
        }
        Ok(None)
    }

    fn refuse_unsafe_checkout_tree(&self, tree_oid: &str) -> Result<(), TactusError> {
        self.validate_tree_oid(tree_oid)?;
        if let Some(problem) = self.tree_input_problem(tree_oid)? {
            return Err(TactusError::Git {
                message: format!("refusing checkout before configured code can run: {problem}"),
            });
        }
        Ok(())
    }

    /// Read the full object ID of the index tree once. Callers that run more
    /// than one verifier can retain this identity and materialize the same
    /// bytes for each verifier even if the source index later changes.
    pub fn staged_tree_oid(&self) -> Result<String, TactusError> {
        let tree = self.git_with_private_hooks(&["write-tree"])?;
        let tree = tree.trim().to_owned();
        self.validate_tree_oid(&tree)?;
        Ok(tree)
    }

    /// A clean detached worktree whose HEAD tree is exactly the staged tree.
    /// Kept for existing callers; new callers that need more than one snapshot
    /// should retain `capture_candidate()` and use
    /// `gate_snapshot_for_candidate()`.
    pub fn gate_snapshot(&self) -> Result<GateWorkspace, TactusError> {
        let parent_oid = self.head_sha_full()?;
        let tree = self.staged_tree_oid()?;
        let observed_parent = self.head_sha_full()?;
        if observed_parent != parent_oid {
            return Err(TactusError::Git {
                message: format!(
                    "HEAD moved from {parent_oid} to {observed_parent} while preparing the gate snapshot"
                ),
            });
        }
        self.gate_snapshot_for_candidate(&parent_oid, &tree)
    }

    /// Materialize a clean detached worktree for one exact tree object ID.
    /// Gates run here, never in the worker's workspace, so ignored files,
    /// build residue, and gate side-effects cannot influence or contaminate the
    /// commit under review.
    pub fn gate_snapshot_for_tree(&self, tree_oid: &str) -> Result<GateWorkspace, TactusError> {
        let parent_oid = self.head_sha_full()?;
        self.gate_snapshot_for_candidate(&parent_oid, tree_oid)
    }

    /// Materialize one frozen candidate. Both object IDs are supplied so a
    /// concurrent ref move cannot silently change the ephemeral commit's
    /// parent after the candidate was reviewed.
    pub fn gate_snapshot_for_candidate(
        &self,
        parent_oid: &str,
        tree_oid: &str,
    ) -> Result<GateWorkspace, TactusError> {
        self.gate_snapshot_for_candidate_in(parent_oid, tree_oid, &std::env::temp_dir())
    }

    fn gate_snapshot_for_candidate_in(
        &self,
        parent_oid: &str,
        tree_oid: &str,
        temp_root: &Path,
    ) -> Result<GateWorkspace, TactusError> {
        self.gate_snapshot_for_candidate_in_with(
            parent_oid,
            tree_oid,
            temp_root,
            |path, hooks_path, commit| self.add_gate_worktree(path, hooks_path, commit),
        )
    }

    fn gate_snapshot_for_candidate_in_with<F>(
        &self,
        parent_oid: &str,
        tree_oid: &str,
        temp_root: &Path,
        add_worktree: F,
    ) -> Result<GateWorkspace, TactusError>
    where
        F: FnOnce(&Path, &Path, &str) -> Result<(), TactusError>,
    {
        self.validate_commit_oid(parent_oid)?;
        self.validate_tree_oid(tree_oid)?;
        if let Some(problem) = self.tree_input_problem(tree_oid)? {
            return Err(TactusError::Git { message: problem });
        }
        let commit = self.git(&[
            "-c",
            "user.name=tactus",
            "-c",
            "user.email=tactus@tactus.local",
            "commit-tree",
            tree_oid,
            "-p",
            parent_oid,
            "-m",
            "[tactus] ephemeral gate snapshot",
        ])?;
        let pending = PendingGateWorkspace::create(&self.root, temp_root)?;
        add_worktree(&pending.path, &pending.hooks_path, commit.trim())?;
        self.verify_gate_worktree(&pending.path, &pending.hooks_path)?;

        // The exact path is already known to be the new worktree's top level.
        // Avoid round-tripping it through Git's textual path output, which is
        // not necessarily UTF-8 on Unix.
        let workspace = Workspace {
            root: pending.path.clone(),
        };
        Ok(pending.finish(workspace))
    }

    fn validate_tree_oid(&self, tree_oid: &str) -> Result<(), TactusError> {
        self.validate_object_oid(tree_oid, "tree")
    }

    fn validate_commit_oid(&self, commit_oid: &str) -> Result<(), TactusError> {
        self.validate_object_oid(commit_oid, "commit")
    }

    fn validate_object_oid(&self, oid: &str, expected_kind: &str) -> Result<(), TactusError> {
        if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(TactusError::Git {
                message: format!("`{oid}` is not a full Git object ID"),
            });
        }
        let kind = self.git(&["cat-file", "-t", oid])?;
        if kind.trim() != expected_kind {
            return Err(TactusError::Git {
                message: format!(
                    "Git object {oid} is a {}, not a {expected_kind}",
                    kind.trim()
                ),
            });
        }
        Ok(())
    }

    fn add_gate_worktree(
        &self,
        path: &Path,
        hooks_path: &Path,
        commit: &str,
    ) -> Result<(), TactusError> {
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(hooks_path);
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .arg("-c")
            .arg(hooks_config)
            .args([
                "-c",
                "core.fsmonitor=false",
                "worktree",
                "add",
                "-q",
                "--detach",
                "--force",
            ])
            .arg(path)
            .arg(commit)
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git worktree add: {e}"),
            })?;
        if !output.status.success() {
            return Err(TactusError::Git {
                message: format!(
                    "git worktree add failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(())
    }

    fn verify_gate_worktree(&self, path: &Path, hooks_path: &Path) -> Result<(), TactusError> {
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(hooks_path);
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("-c")
            .arg(hooks_config)
            .args([
                "-c",
                "core.fsmonitor=false",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ])
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to verify gate worktree: {e}"),
            })?;
        if !output.status.success() {
            return Err(TactusError::Git {
                message: format!(
                    "verifying gate worktree failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        if !output.stdout.is_empty() {
            return Err(TactusError::Git {
                message: format!(
                    "gate worktree materialized with unexpected tracked or untracked state: {}",
                    String::from_utf8_lossy(&output.stdout)
                ),
            });
        }
        Ok(())
    }

    /// Prepare and pin a commit from the exact candidate identities already
    /// used by gates and review. This never rereads the mutable index.
    pub fn prepare_commit_from_candidate(
        &self,
        parent_oid: &str,
        tree_oid: &str,
        message: &str,
        pin_ref: &str,
    ) -> Result<PreparedCommit, TactusError> {
        self.validate_commit_oid(parent_oid)?;
        self.validate_tree_oid(tree_oid)?;
        if self.head_sha_full()? != parent_oid {
            return Err(TactusError::Git {
                message: format!(
                    "HEAD moved after tactus captured candidate parent {parent_oid}; refusing to prepare it"
                ),
            });
        }
        if message.trim().is_empty() || message.contains('\r') || message.contains('\n') {
            return Err(TactusError::Git {
                message: "refusing to prepare a commit with an empty or multi-line subject"
                    .to_owned(),
            });
        }
        self.validate_prepared_ref(pin_ref)?;
        let commit_sha = self
            .commit_tree_with_tactus_identity(tree_oid, parent_oid, message)?
            .trim()
            .to_owned();
        let prepared = PreparedCommit {
            parent_sha: parent_oid.to_owned(),
            tree_sha: tree_oid.to_owned(),
            commit_sha,
            message: message.to_owned(),
            pin_ref: pin_ref.to_owned(),
        };
        if !self.prepared_commit_matches(&prepared)? {
            return Err(TactusError::Git {
                message: "git created a commit object that does not match the prepared identity"
                    .to_owned(),
            });
        }
        let zero = "0".repeat(parent_oid.len());
        self.prepared_update_ref(&[
            "update-ref",
            "-m",
            "tactus: pin prepared task",
            pin_ref,
            &prepared.commit_sha,
            &zero,
        ])?;
        Ok(prepared)
    }

    /// Commit whatever `capture_diff` staged. §14: commit-per-task,
    /// `[tactus] <task-id>: <title>`.
    pub fn commit(&self, message: &str) -> Result<String, TactusError> {
        self.refuse_worktree_filters_before("git commit")?;
        self.git_with_private_hooks(&["commit", "-q", "-m", message])?;
        self.head_sha()
    }

    pub fn prepared_commit_matches(&self, prepared: &PreparedCommit) -> Result<bool, TactusError> {
        if !valid_object_id(&prepared.parent_sha)
            || !valid_object_id(&prepared.tree_sha)
            || !valid_object_id(&prepared.commit_sha)
        {
            return Ok(false);
        }
        if self.validate_prepared_ref(&prepared.pin_ref).is_err() {
            return Ok(false);
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["cat-file", "commit", &prepared.commit_sha])
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        if !output.status.success() {
            return Ok(false);
        }
        let object = String::from_utf8(output.stdout).map_err(|error| TactusError::Git {
            message: format!("prepared commit object is not valid UTF-8: {error}"),
        })?;
        let Some((headers, body)) = object.split_once("\n\n") else {
            return Ok(false);
        };
        let tree = headers.lines().find_map(|line| line.strip_prefix("tree "));
        let parents: Vec<&str> = headers
            .lines()
            .filter_map(|line| line.strip_prefix("parent "))
            .collect();
        let author = headers
            .lines()
            .find_map(|line| line.strip_prefix("author "));
        let committer = headers
            .lines()
            .find_map(|line| line.strip_prefix("committer "));
        Ok(tree == Some(prepared.tree_sha.as_str())
            && parents == [prepared.parent_sha.as_str()]
            && author.is_some_and(|value| value.starts_with("tactus <tactus@tactus.local> "))
            && committer.is_some_and(|value| value.starts_with("tactus <tactus@tactus.local> "))
            && body.trim_end_matches('\n') == prepared.message)
    }

    fn validate_prepared_ref(&self, pin_ref: &str) -> Result<(), TactusError> {
        if !pin_ref.starts_with("refs/tactus/prepared/") {
            return Err(TactusError::Git {
                message: format!("prepared ref `{pin_ref}` is outside tactus's private namespace"),
            });
        }
        self.git(&["check-ref-format", pin_ref]).map(|_| ())
    }

    pub fn prepared_pin_target(&self, pin_ref: &str) -> Result<Option<String>, TactusError> {
        self.validate_prepared_ref(pin_ref)?;
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["rev-parse", "--verify", "--quiet", pin_ref])
            .output()
            .map_err(|e| TactusError::Git {
                message: format!("failed to run git: {e}"),
            })?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ))
    }

    pub fn remove_prepared_pin(&self, prepared: &PreparedCommit) -> Result<(), TactusError> {
        match self.prepared_pin_target(&prepared.pin_ref)? {
            None => Ok(()),
            Some(target) if target == prepared.commit_sha => self.prepared_update_ref(&[
                "update-ref",
                "-d",
                &prepared.pin_ref,
                &prepared.commit_sha,
            ]),
            Some(target) => Err(TactusError::Git {
                message: format!(
                    "prepared ref `{}` points at {target}, not the recorded commit {}; refusing to delete another object",
                    prepared.pin_ref, prepared.commit_sha
                ),
            }),
        }
    }

    /// Remove a private pin for an attempt that never durably recorded a
    /// successful settlement. The target is read then supplied as the expected
    /// old value, so even cleanup is compare-and-swap.
    pub fn remove_orphan_prepared_pin(&self, pin_ref: &str) -> Result<(), TactusError> {
        if let Some(target) = self.prepared_pin_target(pin_ref)? {
            self.prepared_update_ref(&["update-ref", "-d", pin_ref, &target])?;
        }
        Ok(())
    }

    pub fn advance_prepared_commit(&self, prepared: &PreparedCommit) -> Result<(), TactusError> {
        if !self.prepared_commit_matches(prepared)? {
            return Err(TactusError::Git {
                message: "refusing to advance HEAD to a commit that does not match its durable prepared identity".to_owned(),
            });
        }
        if self.prepared_pin_target(&prepared.pin_ref)?.as_deref()
            != Some(prepared.commit_sha.as_str())
        {
            return Err(TactusError::Git {
                message: format!(
                    "prepared ref `{}` does not pin {}; refusing to advance HEAD",
                    prepared.pin_ref, prepared.commit_sha
                ),
            });
        }
        self.prepared_update_ref(&[
            "update-ref",
            "-m",
            "tactus: publish reviewed task",
            "HEAD",
            &prepared.commit_sha,
            &prepared.parent_sha,
        ])?;
        self.remove_prepared_pin(prepared)
    }

    /// Discard everything since the last commit: staged, unstaged, and
    /// untracked (ignored files survive). This is both the §14 rollback on a
    /// failed attempt and the post-commit scrub that keeps gate side-effects
    /// (build artifacts, lockfile churn) from leaking into the next task's
    /// captured diff.
    pub fn discard_uncommitted(&self) -> Result<(), TactusError> {
        let tree_oid = self.git(&["rev-parse", "HEAD^{tree}"])?;
        self.refuse_unsafe_checkout_tree(tree_oid.trim())?;
        self.git_with_private_hooks(&["reset", "-q", "--hard", "HEAD"])?;
        self.git_with_private_hooks(&["clean", "-qfd"]).map(|_| ())
    }
}

struct PrivateHooksDir {
    path: PathBuf,
}

impl PrivateHooksDir {
    fn create() -> Result<Self, TactusError> {
        let path = std::env::temp_dir().join(format!(
            "tactus-empty-hooks-{}-{}",
            std::process::id(),
            crate::ulid::ulid()
        ));
        create_private_dir(&path).map_err(|error| TactusError::Git {
            message: format!(
                "creating private empty hooks directory {}: {error}",
                path.display()
            ),
        })?;
        Ok(Self { path })
    }
}

impl Drop for PrivateHooksDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

struct PendingGateWorkspace {
    source_root: PathBuf,
    path: PathBuf,
    hooks_path: PathBuf,
    armed: bool,
}

impl PendingGateWorkspace {
    fn create(source_root: &Path, temp_root: &Path) -> Result<Self, TactusError> {
        let name = format!(
            "tactus-gates-{}-{}",
            std::process::id(),
            crate::ulid::ulid()
        );
        let path = temp_root.join(&name);
        let hooks_path = temp_root.join(format!("{name}-hooks"));
        create_private_dir(&path).map_err(|e| TactusError::Git {
            message: format!(
                "creating private gate snapshot directory {}: {e}",
                path.display()
            ),
        })?;
        if let Err(error) = create_private_dir(&hooks_path) {
            let _ = fs::remove_dir(&path);
            return Err(TactusError::Git {
                message: format!(
                    "creating private empty hooks directory {}: {error}",
                    hooks_path.display()
                ),
            });
        }
        Ok(Self {
            source_root: source_root.to_path_buf(),
            path,
            hooks_path,
            armed: true,
        })
    }

    fn finish(mut self, workspace: Workspace) -> GateWorkspace {
        self.armed = false;
        GateWorkspace {
            source_root: self.source_root.clone(),
            path: self.path.clone(),
            hooks_path: self.hooks_path.clone(),
            workspace,
        }
    }
}

impl Drop for PendingGateWorkspace {
    fn drop(&mut self) {
        if self.armed {
            cleanup_gate_workspace(&self.source_root, &self.path, &self.hooks_path);
        }
    }
}
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        fs::DirBuilder::new().create(path)
    }
}

fn cleanup_gate_workspace(source_root: &Path, path: &Path, hooks_path: &Path) {
    let mut hooks_config = OsString::from("core.hooksPath=");
    hooks_config.push(hooks_path);
    let _ = Command::new("git")
        .arg("-C")
        .arg(source_root)
        .arg("-c")
        .arg(hooks_config)
        .args([
            "-c",
            "core.fsmonitor=false",
            "worktree",
            "remove",
            "--force",
        ])
        .arg(path)
        .output();
    // `worktree remove` normally removes the directory too. These exact paths
    // were atomically created by this process, so fall back to filesystem
    // cleanup if a partially failed `worktree add` left either one behind.
    let _ = fs::remove_dir_all(path);
    let _ = fs::remove_dir_all(hooks_path);
}

fn valid_object_id(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub struct GateWorkspace {
    source_root: PathBuf,
    path: PathBuf,
    hooks_path: PathBuf,
    workspace: Workspace,
}

impl GateWorkspace {
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
}

impl Drop for GateWorkspace {
    fn drop(&mut self) {
        cleanup_gate_workspace(&self.source_root, &self.path, &self.hooks_path);
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

    fn run_git(repo: &Path, args: &[&str]) -> Vec<u8> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn make_executable(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path).expect("hook metadata").permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            fs::set_permissions(path, permissions).expect("make hook executable");
        }
        #[cfg(not(unix))]
        let _ = path;
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
    fn captured_candidate_keeps_one_parent_tree_and_diff() {
        let repo = temp_repo("captured-candidate");
        let ws = Workspace::open(&repo).expect("open");
        let original_parent = ws.head_sha_full().expect("parent before capture");
        fs::write(repo.join("README.md"), "first candidate\n").expect("first edit");

        let candidate = ws.capture_candidate().expect("capture candidate");
        assert_eq!(candidate.parent_oid, original_parent);
        assert!(
            candidate.diff.contains("first candidate"),
            "{}",
            candidate.diff
        );
        assert_eq!(
            ws.git(&["cat-file", "-t", &candidate.tree_oid])
                .expect("tree type")
                .trim(),
            "tree"
        );

        // Advance the index after capture, then prove the supplied tree still
        // materializes the first candidate rather than rereading that index.
        fs::write(repo.join("README.md"), "second candidate\n").expect("second edit");
        ws.capture_diff().expect("stage second candidate");
        let snapshot = ws
            .gate_snapshot_for_candidate(&candidate.parent_oid, &candidate.tree_oid)
            .expect("materialize frozen tree");
        assert_eq!(
            fs::read_to_string(snapshot.workspace().root().join("README.md"))
                .expect("frozen README")
                .replace("\r\n", "\n"),
            "first candidate\n"
        );
        let snapshot_commit = snapshot
            .workspace()
            .head_sha_full()
            .expect("snapshot commit");
        assert_eq!(
            snapshot
                .workspace()
                .parent_sha(&snapshot_commit)
                .expect("snapshot parent"),
            Some(candidate.parent_oid.clone()),
            "the ephemeral commit must retain the captured parent, not mutable HEAD"
        );

        let error = match ws.gate_snapshot_for_tree(&candidate.parent_oid) {
            Ok(_) => panic!("a commit object is not a supplied tree OID"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("not a tree"), "{error}");
    }

    #[test]
    fn prepared_commit_uses_frozen_objects_identity_and_hook_free_ref_transactions() {
        let repo = temp_repo("prepared");
        let ws = Workspace::open(&repo).expect("open");
        let hook_marker = repo.join("hook-ran");
        ws.git(&["config", "core.hooksPath", ".githooks"])
            .expect("candidate-controlled hooks path");
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        let hook = repo.join(".githooks").join("reference-transaction");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\nprintf ran > '{}'\nexit 1\n",
                hook_marker.display()
            ),
        )
        .expect("hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("executable hook");
        }

        fs::write(repo.join("README.md"), "reviewed candidate\n").expect("candidate");
        let candidate = ws.capture_candidate().expect("freeze candidate");
        fs::write(repo.join("README.md"), "later unreviewed index\n").expect("later edit");
        ws.capture_diff().expect("move index past candidate");

        let pin_ref = "refs/tactus/prepared/01RUN/0-1";
        let prepared = ws
            .prepare_commit_from_candidate(
                &candidate.parent_oid,
                &candidate.tree_oid,
                "[tactus] t1: task",
                pin_ref,
            )
            .expect("commit-tree and pin creation ignore candidate hooks");
        assert_eq!(ws.head_sha_full().expect("head"), candidate.parent_oid);
        assert_eq!(
            ws.prepared_pin_target(pin_ref).expect("pin").as_deref(),
            Some(prepared.commit_sha.as_str()),
            "the object is reachable before settlement"
        );
        let object = ws
            .git(&["cat-file", "commit", &prepared.commit_sha])
            .expect("prepared object");
        assert!(
            object.contains("author tactus <tactus@tactus.local> "),
            "{object}"
        );
        assert!(
            object.contains("committer tactus <tactus@tactus.local> "),
            "{object}"
        );
        assert_eq!(
            ws.git(&["show", &format!("{}:README.md", prepared.commit_sha)])
                .expect("frozen blob"),
            "reviewed candidate\n",
            "preparation never rereads the later index"
        );
        assert!(!hook_marker.exists(), "pin creation never ran the ref hook");

        ws.advance_prepared_commit(&prepared).expect("HEAD CAS");
        assert_eq!(
            ws.head_sha_full().expect("advanced head"),
            prepared.commit_sha
        );
        assert_eq!(ws.prepared_pin_target(pin_ref).expect("deleted pin"), None);
        assert!(
            !hook_marker.exists(),
            "neither HEAD publication nor pin deletion ran the ref hook"
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
        let error = ws
            .capture_diff()
            .expect_err("filters must be refused before staging")
            .to_string();
        assert!(error.contains("before git add"), "{error}");
        assert!(error.contains("filtered.txt"), "{error}");

        // Preserve the independent post-stage guard for a caller opening an
        // index prepared outside Workspace::capture_candidate.
        run_git(&repo, &["add", "-A"]);
        let filtered_tree = ws.staged_tree_oid().expect("filtered tree");
        fs::write(repo.join(".gitattributes"), "").expect("clear live attributes");
        run_git(&repo, &["add", ".gitattributes"]);

        let problem = ws
            .review_input_problem_for_tree(&filtered_tree)
            .expect("inspect attributes")
            .expect("filtered evidence must fail closed");
        assert!(problem.contains("filtered.txt"), "{problem}");
        assert!(problem.contains("tactus-test"), "{problem}");
        assert!(problem.contains("different bytes"), "{problem}");
    }

    #[test]
    fn filter_on_unchanged_tracked_path_is_refused_before_materialization() {
        let repo = temp_repo("filter-on-unchanged-path");
        let ws = Workspace::open(&repo).expect("open");
        fs::write(repo.join("unchanged.txt"), "tracked baseline\n").expect("tracked file");
        ws.capture_diff().expect("stage baseline file");
        ws.commit("seed unchanged file")
            .expect("commit baseline file");

        // Only the attributes file changes. The filter target itself is absent
        // from `diff --cached --name-only` but is still a gate input.
        fs::write(
            repo.join(".gitattributes"),
            "unchanged.txt filter=tactus-test\n",
        )
        .expect("candidate attributes");
        let error = ws
            .capture_candidate()
            .expect_err("unchanged filtered targets must fail before add")
            .to_string();
        assert!(error.contains("unchanged.txt"), "{error}");
        run_git(&repo, &["add", "-A"]);
        let tree_oid = ws.staged_tree_oid().expect("externally captured tree");
        fs::remove_file(repo.join(".gitattributes")).expect("move index past candidate");
        run_git(&repo, &["add", "-A"]);

        let problem = ws
            .review_input_problem_for_tree(&tree_oid)
            .expect("inspect every path in the captured tree")
            .expect("filter on unchanged path must fail closed");
        assert!(problem.contains("unchanged.txt"), "{problem}");
        assert!(problem.contains("tactus-test"), "{problem}");
    }

    #[test]
    fn capture_candidate_refuses_filter_before_candidate_helper_executes() {
        let repo = temp_repo("pre-add-filter-helper");
        fs::create_dir_all(repo.join(".githooks")).expect("helper directory");
        fs::write(
            repo.join(".githooks").join("filter-helper"),
            "#!/bin/sh\ncat\n",
        )
        .expect("baseline helper");
        fs::write(repo.join("payload.txt"), "baseline\n").expect("payload");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "seed filter helper"]);
        run_git(
            &repo,
            &[
                "config",
                "filter.tactus-test.clean",
                "sh .githooks/filter-helper",
            ],
        );
        run_git(&repo, &["config", "filter.tactus-test.smudge", "cat"]);

        fs::write(
            repo.join(".githooks").join("filter-helper"),
            "#!/bin/sh\nprintf 'ran\\n' > filter-ran\ncat\n",
        )
        .expect("candidate helper");
        fs::write(
            repo.join(".gitattributes"),
            "payload.txt filter=tactus-test\n",
        )
        .expect("candidate attributes");
        fs::write(repo.join("payload.txt"), "candidate\n").expect("candidate payload");
        let ws = Workspace::open(&repo).expect("open");

        let error = ws
            .capture_candidate()
            .expect_err("filter must be refused before git add")
            .to_string();
        assert!(error.contains("before git add"), "{error}");
        assert!(error.contains("payload.txt"), "{error}");
        assert!(
            !repo.join("filter-ran").exists(),
            "attribute inspection must not execute the candidate-edited filter helper"
        );

        // Control: the exact raw command that capture used to run executes the
        // fixture, proving marker absence above is suppression rather than a
        // helper that could never run on this platform.
        run_git(&repo, &["add", "-A"]);
        assert!(repo.join("filter-ran").exists(), "raw git add ran filter");
    }

    #[test]
    fn status_and_switch_refuse_filter_before_candidate_helper_executes() {
        let repo = temp_repo("status-filter-helper");
        fs::create_dir_all(repo.join(".githooks")).expect("helper directory");
        fs::write(
            repo.join(".githooks").join("status-filter"),
            "#!/bin/sh\ncat\n",
        )
        .expect("baseline helper");
        fs::write(repo.join("payload.txt"), "baseline\n").expect("payload");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "seed status helper"]);
        run_git(&repo, &["branch", "alternate"]);
        run_git(&repo, &["config", "core.trustctime", "false"]);
        run_git(&repo, &["config", "core.checkStat", "minimal"]);
        run_git(
            &repo,
            &[
                "config",
                "filter.tactus-status.clean",
                "sh .githooks/status-filter",
            ],
        );
        run_git(&repo, &["config", "filter.tactus-status.smudge", "cat"]);

        fs::write(
            repo.join(".githooks").join("status-filter"),
            "#!/bin/sh\nprintf 'ran\\n' > status-filter-ran\ncat\n",
        )
        .expect("candidate helper");
        fs::write(
            repo.join(".gitattributes"),
            "payload.txt filter=tactus-status\n",
        )
        .expect("candidate attributes");
        let payload = repo.join("payload.txt");
        let indexed_mtime = fs::metadata(&payload)
            .and_then(|metadata| metadata.modified())
            .expect("indexed payload mtime");
        fs::write(&payload, "changed!\n").expect("same-size candidate payload");
        fs::OpenOptions::new()
            .write(true)
            .open(&payload)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(indexed_mtime)))
            .expect("restore indexed mtime");
        fs::OpenOptions::new()
            .write(true)
            .open(repo.join(".git").join("index"))
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(indexed_mtime)))
            .expect("force a deterministic racily-clean index comparison");
        let ws = Workspace::open(&repo).expect("open");

        let clean_error = ws
            .is_clean()
            .expect_err("status preflight must refuse the filter")
            .to_string();
        assert!(clean_error.contains("before git status"), "{clean_error}");
        let summary_error = ws
            .uncommitted_summary()
            .expect_err("resume summary must refuse the filter")
            .to_string();
        assert!(
            summary_error.contains("before git status"),
            "{summary_error}"
        );
        let head_tree = ws.git(&["rev-parse", "HEAD^{tree}"]).expect("head tree");
        let review_problem = ws
            .review_input_problem_for_tree(head_tree.trim())
            .expect("review preflight")
            .expect("review status must refuse the filter");
        assert!(
            review_problem.contains("before git status"),
            "{review_problem}"
        );
        let switch_error = ws
            .switch_branch("alternate")
            .expect_err("branch switch must refuse the live filter")
            .to_string();
        assert!(switch_error.contains("before git switch"), "{switch_error}");
        let commit_error = ws
            .commit("must not inspect filtered worktree")
            .expect_err("commit must refuse the live filter")
            .to_string();
        assert!(commit_error.contains("before git commit"), "{commit_error}");
        assert!(
            !repo.join("status-filter-ran").exists(),
            "preflight inspection must not execute the candidate-edited filter helper"
        );

        run_git(&repo, &["status", "--porcelain"]);
        assert!(
            repo.join("status-filter-ran").exists(),
            "raw git status ran the candidate-edited clean filter"
        );
    }

    #[test]
    fn capture_candidate_disables_candidate_fsmonitor() {
        let repo = temp_repo("capture-fsmonitor");
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        fs::write(
            repo.join(".githooks").join("fsmonitor"),
            "#!/bin/sh\nprintf 'baseline-token\\0'\n",
        )
        .expect("baseline fsmonitor");
        make_executable(&repo.join(".githooks").join("fsmonitor"));
        run_git(&repo, &["add", "-A"]);
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/fsmonitor"],
        );
        run_git(&repo, &["commit", "-q", "-m", "seed fsmonitor"]);
        run_git(&repo, &["config", "core.fsmonitor", ".githooks/fsmonitor"]);
        run_git(&repo, &["config", "core.fsmonitorHookVersion", "2"]);
        run_git(&repo, &["status", "--porcelain"]);

        fs::write(
            repo.join(".githooks").join("fsmonitor"),
            "#!/bin/sh\nprintf 'ran\\n' > fsmonitor-ran\nprintf 'candidate-token\\0'\n",
        )
        .expect("candidate fsmonitor");
        fs::write(repo.join("README.md"), "candidate\n").expect("candidate edit");
        let ws = Workspace::open(&repo).expect("open without fsmonitor execution");
        ws.capture_candidate()
            .expect("capture with fsmonitor explicitly disabled");
        assert!(!repo.join("fsmonitor-ran").exists());
        assert!(!ws.is_clean().expect("status with fsmonitor disabled"));
        assert!(!repo.join("fsmonitor-ran").exists());

        fs::write(repo.join("README.md"), "control\n").expect("control edit");
        run_git(&repo, &["add", "-A"]);
        assert!(
            repo.join("fsmonitor-ran").exists(),
            "raw git add ran the candidate-edited fsmonitor"
        );
    }

    #[test]
    fn capture_candidate_disables_post_index_change_hook() {
        let repo = temp_repo("capture-post-index-change");
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        fs::write(
            repo.join(".githooks").join("post-index-change"),
            "#!/bin/sh\nexit 0\n",
        )
        .expect("baseline hook");
        make_executable(&repo.join(".githooks").join("post-index-change"));
        run_git(&repo, &["add", "-A"]);
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/post-index-change"],
        );
        run_git(&repo, &["commit", "-q", "-m", "seed index hook"]);
        run_git(&repo, &["config", "core.hooksPath", ".githooks"]);

        fs::write(
            repo.join(".githooks").join("post-index-change"),
            "#!/bin/sh\nprintf 'ran\\n' > post-index-ran\n",
        )
        .expect("candidate hook");
        fs::write(repo.join("README.md"), "candidate\n").expect("candidate edit");
        let ws = Workspace::open(&repo).expect("open");
        ws.capture_candidate()
            .expect("capture with private empty hooks path");
        assert!(!repo.join("post-index-ran").exists());

        fs::write(repo.join("README.md"), "control\n").expect("control edit");
        run_git(&repo, &["add", "-A"]);
        assert!(
            repo.join("post-index-ran").exists(),
            "raw git add ran post-index-change"
        );
    }

    #[test]
    fn branch_creation_and_switch_do_not_execute_post_checkout_hook() {
        let repo = temp_repo("branch-post-checkout");
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        fs::write(
            repo.join(".githooks").join("post-checkout"),
            "#!/bin/sh\nprintf 'ran\\n' > post-checkout-ran\n",
        )
        .expect("checkout hook");
        make_executable(&repo.join(".githooks").join("post-checkout"));
        run_git(&repo, &["add", "-A"]);
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/post-checkout"],
        );
        run_git(&repo, &["commit", "-q", "-m", "seed checkout hook"]);
        run_git(&repo, &["config", "core.hooksPath", ".githooks"]);
        let ws = Workspace::open(&repo).expect("open");

        ws.create_branch("safe-create")
            .expect("hook-suppressed branch creation");
        assert!(!repo.join("post-checkout-ran").exists());
        ws.switch_branch("main")
            .expect("hook-suppressed branch switch");
        assert!(!repo.join("post-checkout-ran").exists());

        run_git(&repo, &["switch", "-q", "safe-create"]);
        assert!(
            repo.join("post-checkout-ran").exists(),
            "raw git switch ran post-checkout"
        );
    }

    #[test]
    fn branch_switch_refuses_filter_before_target_helper_executes() {
        let repo = temp_repo("branch-filter-helper");
        fs::create_dir_all(repo.join(".githooks")).expect("helper directory");
        fs::write(
            repo.join(".githooks").join("smudge-helper"),
            "#!/bin/sh\nprintf 'ran\\n' > smudge-ran\ncat\n",
        )
        .expect("smudge helper");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "seed smudge helper"]);
        run_git(&repo, &["switch", "-q", "-c", "filtered"]);
        fs::write(
            repo.join(".gitattributes"),
            "payload.txt filter=tactus-switch\n",
        )
        .expect("attributes");
        fs::write(repo.join("payload.txt"), "filtered branch\n").expect("payload");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "seed filtered branch"]);
        run_git(&repo, &["switch", "-q", "main"]);
        run_git(&repo, &["config", "filter.tactus-switch.clean", "cat"]);
        run_git(
            &repo,
            &[
                "config",
                "filter.tactus-switch.smudge",
                "sh .githooks/smudge-helper",
            ],
        );
        let ws = Workspace::open(&repo).expect("open");

        let error = ws
            .switch_branch("filtered")
            .expect_err("target filters must fail before checkout")
            .to_string();
        assert!(error.contains("refusing checkout"), "{error}");
        assert!(error.contains("payload.txt"), "{error}");
        assert!(!repo.join("smudge-ran").exists());
        assert_eq!(ws.current_branch().expect("still on main"), "main");

        run_git(&repo, &["switch", "-q", "filtered"]);
        assert!(
            repo.join("smudge-ran").exists(),
            "raw git switch ran the target's configured smudge helper"
        );
    }

    #[test]
    fn commit_and_discard_do_not_execute_candidate_hooks() {
        let repo = temp_repo("commit-reset-hooks");
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        fs::write(
            repo.join(".githooks").join("pre-commit"),
            "#!/bin/sh\nprintf 'ran\\n' > pre-commit-ran\n",
        )
        .expect("commit hook");
        fs::write(
            repo.join(".githooks").join("post-index-change"),
            "#!/bin/sh\nprintf 'ran\\n' > reset-index-ran\n",
        )
        .expect("index hook");
        make_executable(&repo.join(".githooks").join("pre-commit"));
        make_executable(&repo.join(".githooks").join("post-index-change"));
        run_git(&repo, &["add", "-A"]);
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/pre-commit"],
        );
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/post-index-change"],
        );
        run_git(&repo, &["commit", "-q", "-m", "seed hooks"]);
        run_git(&repo, &["config", "core.hooksPath", ".githooks"]);
        let ws = Workspace::open(&repo).expect("open");

        fs::write(repo.join("README.md"), "candidate commit\n").expect("edit");
        ws.capture_candidate().expect("capture");
        assert!(!repo.join("reset-index-ran").exists());
        ws.commit("candidate without hooks")
            .expect("hook-suppressed commit");
        assert!(!repo.join("pre-commit-ran").exists());

        fs::write(repo.join("README.md"), "candidate reset\n").expect("reset edit");
        ws.capture_candidate().expect("capture reset candidate");
        ws.discard_uncommitted()
            .expect("hook-suppressed reset and clean");
        assert!(!repo.join("reset-index-ran").exists());

        fs::write(repo.join("README.md"), "control\n").expect("control edit");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "control commit"]);
        assert!(repo.join("pre-commit-ran").exists(), "raw commit ran hook");
    }

    #[test]
    fn discard_refuses_target_filter_before_candidate_helper_executes() {
        let repo = temp_repo("reset-filter-helper");
        fs::write(
            repo.join(".gitattributes"),
            "README.md filter=tactus-reset\n",
        )
        .expect("target attributes");
        fs::write(repo.join("zz-reset-helper"), "#!/bin/sh\ncat\n").expect("baseline helper");
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "seed reset filter"]);
        run_git(&repo, &["config", "filter.tactus-reset.clean", "cat"]);
        run_git(
            &repo,
            &["config", "filter.tactus-reset.smudge", "sh zz-reset-helper"],
        );

        fs::write(
            repo.join("zz-reset-helper"),
            "#!/bin/sh\nprintf 'ran\\n' > reset-filter-ran\ncat\n",
        )
        .expect("candidate helper");
        fs::write(repo.join("README.md"), "candidate\n").expect("candidate payload");
        let ws = Workspace::open(&repo).expect("open");

        let error = ws
            .discard_uncommitted()
            .expect_err("reset target filters must fail before checkout")
            .to_string();
        assert!(error.contains("refusing checkout"), "{error}");
        assert!(error.contains("README.md"), "{error}");
        assert!(
            !repo.join("reset-filter-ran").exists(),
            "tree inspection must not execute the candidate-edited smudge helper"
        );

        run_git(&repo, &["reset", "-q", "--hard", "HEAD"]);
        assert!(
            repo.join("reset-filter-ran").exists(),
            "raw git reset ran the candidate-edited smudge helper"
        );
    }

    #[test]
    fn gate_snapshot_does_not_execute_post_checkout_hook() {
        let repo = temp_repo("snapshot-checkout-hook");
        run_git(&repo, &["config", "core.autocrlf", "false"]);
        run_git(&repo, &["config", "core.hooksPath", ".githooks"]);
        fs::create_dir_all(repo.join(".githooks")).expect("hooks directory");
        fs::write(
            repo.join(".githooks").join("post-checkout"),
            "#!/bin/sh\nprintf 'ran\\n' > hook-ran\n",
        )
        .expect("candidate checkout hook");
        let ws = Workspace::open(&repo).expect("open");
        ws.capture_diff().expect("stage candidate hook");
        run_git(
            &repo,
            &["update-index", "--chmod=+x", ".githooks/post-checkout"],
        );

        let snapshot = ws.gate_snapshot().expect("hook-suppressed snapshot");
        assert!(
            snapshot
                .workspace()
                .root()
                .join(".githooks")
                .join("post-checkout")
                .exists(),
            "the candidate hook itself remains part of the reviewed tree"
        );
        assert!(
            !snapshot.workspace().root().join("hook-ran").exists(),
            "materialization must never execute candidate-controlled checkout hooks"
        );
        assert!(
            fs::read_dir(&snapshot.hooks_path)
                .expect("private hooks directory")
                .next()
                .is_none(),
            "the override must point at a private empty directory"
        );
    }

    #[test]
    fn failed_gate_snapshot_add_cleans_registered_worktree() {
        let repo = temp_repo("failed-snapshot-add-cleanup");
        let ws = Workspace::open(&repo).expect("open");
        let parent = ws.head_sha_full().expect("parent");
        let tree = ws.staged_tree_oid().expect("tree");
        let registrations_before = ws
            .git(&["worktree", "list", "--porcelain"])
            .expect("registrations before");
        let temp_root = env::temp_dir();
        let mut attempted_path = None;
        let mut attempted_hooks_path = None;

        let result = ws.gate_snapshot_for_candidate_in_with(
            &parent,
            &tree,
            &temp_root,
            |path, hooks_path, commit| {
                attempted_path = Some(path.to_path_buf());
                attempted_hooks_path = Some(hooks_path.to_path_buf());
                // Model the dangerous failure boundary: Git has registered and
                // populated the worktree, then the overall add operation is
                // reported as failed (as a failing post-checkout hook did).
                ws.add_gate_worktree(path, hooks_path, commit)?;
                Err(TactusError::Git {
                    message: "synthetic late worktree-add failure".to_owned(),
                })
            },
        );
        let error = match result {
            Ok(_) => panic!("synthetic worktree-add failure must propagate"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("synthetic late"), "{error}");

        let attempted_path = attempted_path.expect("attempted snapshot path");
        let attempted_hooks_path = attempted_hooks_path.expect("attempted hooks path");
        assert!(!attempted_path.exists(), "snapshot directory cleaned");
        assert!(!attempted_hooks_path.exists(), "hooks directory cleaned");
        assert_eq!(
            ws.git(&["worktree", "list", "--porcelain"])
                .expect("registrations after"),
            registrations_before,
            "a failed add must not leave a registered worktree"
        );
    }

    #[test]
    fn unexpected_materialization_residue_is_rejected_and_cleaned() {
        let repo = temp_repo("snapshot-residue-cleanup");
        let ws = Workspace::open(&repo).expect("open");
        let parent = ws.head_sha_full().expect("parent");
        let tree = ws.staged_tree_oid().expect("tree");
        let registrations_before = ws
            .git(&["worktree", "list", "--porcelain"])
            .expect("registrations before");
        let temp_root = env::temp_dir();
        let mut attempted_path = None;

        let result = ws.gate_snapshot_for_candidate_in_with(
            &parent,
            &tree,
            &temp_root,
            |path, hooks_path, commit| {
                attempted_path = Some(path.to_path_buf());
                ws.add_gate_worktree(path, hooks_path, commit)?;
                fs::write(path.join("unexpected-residue"), "not in candidate\n").map_err(
                    |error| TactusError::Git {
                        message: format!("creating synthetic residue: {error}"),
                    },
                )?;
                Ok(())
            },
        );
        let error = match result {
            Ok(_) => panic!("unexpected materialization residue must fail closed"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("unexpected tracked or untracked state"),
            "{error}"
        );
        assert!(
            !attempted_path.expect("attempted path").exists(),
            "rejected snapshot directory cleaned"
        );
        assert_eq!(
            ws.git(&["worktree", "list", "--porcelain"])
                .expect("registrations after"),
            registrations_before,
            "rejected materialization must not stay registered"
        );
    }

    #[cfg(unix)]
    #[test]
    fn gate_snapshot_target_is_atomically_private() {
        use std::os::unix::fs::PermissionsExt;

        let repo = temp_repo("private-snapshot-target");
        let pending = PendingGateWorkspace::create(&repo, &env::temp_dir())
            .expect("atomically create private snapshot directories");
        let path = pending.path.clone();
        let hooks_path = pending.hooks_path.clone();
        assert_eq!(
            fs::metadata(&path)
                .expect("snapshot metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&hooks_path)
                .expect("hooks metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(fs::read_dir(&path).expect("empty target").next().is_none());
        assert!(
            fs::read_dir(&hooks_path)
                .expect("empty hooks path")
                .next()
                .is_none()
        );
        drop(pending);
        assert!(!path.exists());
        assert!(!hooks_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn gate_snapshot_accepts_non_utf8_tmpdir_on_unix() {
        use std::os::unix::ffi::OsStringExt;

        let repo = temp_repo("non-utf8-snapshot-root");
        let ws = Workspace::open(&repo).expect("open");
        let parent = ws.head_sha_full().expect("parent");
        let tree = ws.staged_tree_oid().expect("tree");
        let mut name = format!("tactus-non-utf8-tmp-{}-", std::process::id()).into_bytes();
        name.push(0xff);
        let temp_root = env::temp_dir().join(OsString::from_vec(name));
        let _ = fs::remove_dir_all(&temp_root);
        fs::create_dir(&temp_root).expect("non-UTF-8 temp root");

        let snapshot = ws
            .gate_snapshot_for_candidate_in(&parent, &tree, &temp_root)
            .expect("Path/OsStr must reach git without UTF-8 conversion");
        assert!(snapshot.workspace().root().starts_with(&temp_root));
        assert!(snapshot.workspace().is_clean().expect("clean snapshot"));
        drop(snapshot);
        fs::remove_dir(&temp_root).expect("clean non-UTF-8 temp root");
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

    #[test]
    fn clean_unchanged_submodule_is_refused_before_gate_snapshot() {
        let child = temp_repo("clean-submodule-child");
        let repo = temp_repo("clean-submodule-parent");
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "protocol.file.allow=always", "submodule", "add", "-q"])
            .arg(&child)
            .arg("nested")
            .output()
            .expect("add submodule");
        assert!(
            output.status.success(),
            "submodule add: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let ws = Workspace::open(&repo).expect("open parent");
        ws.capture_diff().expect("stage submodule");
        ws.commit("seed clean submodule").expect("commit submodule");
        assert!(ws.is_clean().expect("clean parent and submodule"));

        let problem = ws
            .review_input_problem()
            .expect("inspect complete index")
            .expect("even a clean unchanged gitlink must fail closed");
        assert!(problem.contains("nested"), "{problem}");
        assert!(problem.contains("mode 160000"), "{problem}");

        let tree = ws.staged_tree_oid().expect("indexed tree");
        let error = match ws.gate_snapshot_for_tree(&tree) {
            Ok(_) => panic!("a gitlink tree must not be materialized incompletely"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("nested"), "{error}");
        assert!(error.contains("mode 160000"), "{error}");
    }
}
