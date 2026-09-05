# `src/workspace.rs`

Extended notes for [`src/workspace.rs`](../../src/workspace.rs).

The code is the authority for what it does. This file preserves the migrated prose. Each section
names the source item and, where needed, the line its comment described. Each code snippet in a
heading is a literal source lookup string.

The source retains the `LEGACY-EFFECT` allowlist-placement marker above its governed lint
allowance. Lint reason strings stay in their attributes.

## Module

Workspace (DESIGN.md §6): the engine owns git. Agents edit files; only the
engine stages, commits, branches, and rolls back (invariant 1). Every git
operation is a subprocess of the system `git` binary — no library binding.

### LEGACY-EFFECT

`decisions.effect_site_inventory.mechanism` puts this module in the **frozen
legacy section** of `effects/allowlist.toml` by name: "legacy modules frozen
at PR5 (… legacy branch/checkout/commit operations in src/workspace.rs …)
each carrying a LEGACY-EFFECT justification". The justification is that
sentence's own: these are the schema-1..3 engine's Git operations, they are
reached only by legacy paths, and `invariants_preserved[1]` requires their
behaviour to be untouched by this slice. The schema-4 primitives —
execution root, detached worktrees with intents, exact snapshots, engine
refs, and the Git-object creation contexts — live behind typed funnels in
[`crate::workspace_manager`] instead, and nothing here calls them.

The section "may only shrink after PR5 (the test compares against the frozen
list)", so this attribute is a ceiling rather than a licence.

## `pub struct CapturedCandidate {`

The immutable candidate captured immediately after staging. Every gate,
review, prepared commit, and CAS uses these object identities rather than
consulting a mutable index again.

## `pub struct CapturedCandidate` › `pub branch_ref: String,`

The exact direct branch ref that owned `parent_oid` when this candidate
was captured. An object id alone is insufficient: two branches may
legitimately point at the same commit while only one belongs to the run.

## `pub(crate) const REVIEW_DIFF_FLAGS: &[&str] = &[`

The fixed arguments of a reviewable diff, before its two revisions.

**Shared because there are now two callers and one meaning.** Schemas 1–3
capture the diff from the task workspace ([`Workspace::capture_candidate`]);
the schema-4 driver captures a tree and asks
[`crate::workspace_manager::WorkspaceManager::candidate_diff`] for the diff
of that tree against its parent. Both produce the text a reviewer judges and
`classify::diff_failure` reads, so both must be the *same* text.

Every flag is load-bearing and each one defends against operator config
rather than against Git's defaults. A configured `diff.external`
(difftastic and friends) replaces the output wholesale; `color.ui` injects
escape codes; `textconv` substitutes a rendered form for the bytes. Any of
those corrupts every downstream check that reads the diff — and
`capture_diff_is_immune_to_user_diff_config` is the test that says so.

## `impl Workspace` › `pub fn open(root: &Path) -> Result<Self, UpstrokeError> {`

Open an existing git worktree, normalizing to its top level. Running
from a subdirectory would otherwise scope `git clean` to that
subdirectory while staging stays whole-tree, so rollback would leave
residue above the current directory.

## `impl Workspace` › `pub(crate) fn worktree_git_dir(&self) -> Result<PathBuf, UpstrokeError> {`

The administrative directory private to this physical worktree.

A linked worktree's `.git` is a pointer into the common repository, so
joining the visible `.git` path would either fail or collapse distinct
worktrees onto one lease. Git resolves the exact per-worktree directory
for us without changing tracked or working-tree state.

## `impl Workspace` › `fn git_path(&self, args: &[&str]) -> Result<PathBuf, UpstrokeError> {`

Decode one path printed by Git without requiring Unix path bytes to be
UTF-8. Git appends a platform line ending; remove only that delimiter,
never legal leading or trailing path bytes.

## `impl Workspace` › `pub(crate) fn run_git_with_private_hooks(`

Run a Git command with every repository-configured hook and fsmonitor
disabled. Keep this raw-output primitive reusable by reference updates,
whose expected compare-and-swap failures need the real exit status.

## `impl Workspace` › `let writer = std::thread::spawn(move || stdin.write_all(&input));`

Read stdout/stderr while feeding the complete NUL-delimited path
list. A large index can otherwise fill check-attr's stdout pipe and
deadlock the parent while it is still writing stdin.

## `impl Workspace` › `.env("GIT_AUTHOR_NAME", "upstroke")`

Environment identity overrides repository/global config and any
inherited GIT_AUTHOR_* or GIT_COMMITTER_* values.

## `impl Workspace` › `pub fn is_clean(&self) -> Result<bool, UpstrokeError> {`

§14 pre-flight: the engine refuses dirty trees.

## `impl Workspace` › `pub fn ensure_execution_prerequisites(&self) -> Result<(), UpstrokeError> {`

Repository prerequisites whose absence would make the captured tree
incomplete or its attribute policy unverifiable. Run this before any
worker is dispatched on both fresh and resumed runs.

## `fn refuse_sparse_checkout(&self) -> Result<(), UpstrokeError> {` › `let index = self.git_output_with_private_hooks(&["ls-files", "-t", "-z"])?;`

`-t` reports the skip-worktree tag as an uppercase `S` even when an
entry is also marked assume-unchanged. (`-v` would lowercase that
tag and could let a manually sparse index evade this preflight.)

## `impl Workspace` › `pub fn current_branch_ref(&self) -> Result<String, UpstrokeError> {`

The full direct branch ref currently checked out by this worktree.
Prepared publication is deliberately unavailable from detached HEAD or
through a symbolic branch alias: the run records one concrete local ref.

## `impl Workspace` › `pub fn head_sha_full(&self) -> Result<String, UpstrokeError> {`

Full HEAD sha. The event log records these rather than short ones
because `--short` picks its length from `core.abbrev` and the repo's
object count — a sha written by one checkout would not compare equal to
the same sha read by another, which is exactly the check §15 asks
`resume` to make.

## `impl Workspace` › `pub fn parent_sha(&self, sha: &str) -> Result<Option<String>, UpstrokeError> {`

The full sha of a commit's first parent — `None` at a root commit.

How `resume` tells a commit sitting directly on its own record apart
from history that arrived some other way.

## `pub fn parent_sha(&self, sha: &str) -> Result<Option<String>, UpstrokeError> {` › `return Ok(None);`

A root commit has no parent. That is an answer, not a failure.

## `impl Workspace` › `pub fn commit_subject(&self, sha: &str) -> Result<String, UpstrokeError> {`

A commit's subject — the first line of its message.

## `impl Workspace` › `pub fn switch_branch(&self, name: &str) -> Result<(), UpstrokeError> {`

Move to an existing branch — how `resume` gets back onto the run's own
branch when the operator has wandered off it.

## `impl Workspace` › `pub fn branch_exists(&self, name: &str) -> Result<bool, UpstrokeError> {`

Whether a branch exists locally.

## `impl Workspace` › `pub fn uncommitted_summary(&self) -> Result<Vec<String>, UpstrokeError> {`

A one-line-per-path summary of everything uncommitted, for telling the
operator what a resume is about to discard.

## `impl Workspace` › `pub fn ensure_run_exclusions(&self) -> Result<(), UpstrokeError> {`

Keep `.upstroke/` (run dirs, transcripts) out of `status` and out of the
engine's own commits.

This is a self-ignoring `.upstroke/.gitignore` containing `*` (the
pattern cargo uses for `target/`) rather than an entry in
`.git/info/exclude`: it needs no read-modify-write of a file the user
owns, disappears with the directory, and — unlike `info/exclude` under
`--git-dir` — behaves correctly in a linked worktree, where git reads
excludes only from the common directory.

## `impl Workspace` › `pub fn capture_candidate(&self) -> Result<CapturedCandidate, UpstrokeError> {`

Stage everything, freeze one parent and tree object, and return their
complete diff. The diff names those frozen objects rather than rereading
HEAD or the index, so all three values remain one candidate even if a
ref or the index changes afterward.

The diff must be a plain unified diff regardless of user config: a
configured `diff.external` (difftastic and friends) would replace it
wholesale and `color.ui` would inject escape codes, corrupting every
downstream check that reads it.

## `impl Workspace` › `pub fn capture_diff(&self) -> Result<String, UpstrokeError> {`

Backward-compatible diff-only capture for existing callers.

## `fn worktree_filter_problem(&self, operation: &str) -> Result<Option<String>, UpstrokeError> {` › `let paths = self.git_output_with_private_hooks(&[`

Commands that inspect or update worktree entries (`add`, `status`,
`switch`, `commit`) can run clean/process filters before a later
tree policy check. Enumerate tracked and addable untracked paths
without refreshing fsmonitor, then evaluate the worktree's
attributes without invoking a driver.

## `impl Workspace` › `pub fn review_input_problem(&self) -> Result<Option<String>, UpstrokeError> {`

Refuse staged evidence whose bytes are not the bytes a gate would see,
or whose worktree still contains unstaged nested state after `git add`.
A clean/smudge filter makes the cached diff describe the transformed
blob while gates see the smudged file. Dirty submodules similarly hide
executable inputs behind an unchanged gitlink. Neither can be reviewed
completely, so both are policy failures rather than gate results.

## `impl Workspace` › `pub fn review_input_problem_for_tree(`

Inspect live nested-worktree state, then bind every semantic input check
to one captured tree rather than to an index that may have moved since
its diff was produced.

## `fn tree_input_problem(&self, tree_oid: &str) -> Result<Option<String>, UpstrokeError> {` › `let entries = self.git_output(&["ls-tree", "-r", "-z", "--full-tree", tree_oid])?;`

A captured .gitattributes can attach a filter to an otherwise
unchanged file, so changed names are insufficient. `ls-tree`
enumerates every path in the exact candidate and exposes gitlinks.

## `impl Workspace` › `pub fn staged_tree_oid(&self) -> Result<String, UpstrokeError> {`

Read the full object ID of the index tree once. Callers that run more
than one verifier can retain this identity and materialize the same
bytes for each verifier even if the source index later changes.

## `impl Workspace` › `pub fn gate_snapshot(&self) -> Result<GateWorkspace, UpstrokeError> {`

A clean detached worktree whose HEAD tree is exactly the staged tree.
Kept for existing callers; new callers that need more than one snapshot
should retain `capture_candidate()` and use
`gate_snapshot_for_candidate()`.

## `impl Workspace` › `pub fn gate_snapshot_for_tree(&self, tree_oid: &str) -> Result<GateWorkspace, UpstrokeError> {`

Materialize a clean detached worktree for one exact tree object ID.
Gates run here, never in the worker's workspace, so ignored files,
build residue, and gate side-effects cannot influence or contaminate the
commit under review.

## `impl Workspace` › `pub fn gate_snapshot_for_candidate(`

Materialize one frozen candidate. Both object IDs are supplied so a
concurrent ref move cannot silently change the ephemeral commit's
parent after the candidate was reviewed.

## `impl Workspace` › `pub fn gate_snapshot_for_candidate_in_store(`

Materialize a candidate under a durable, caller-owned snapshot store.
The intent is synced before Git registers the worktree, allowing resume
to reclaim a snapshot whose owner was terminated without running Drop.

## `impl Workspace` › `pub fn reclaim_gate_workspaces(&self, store: &Path) -> Result<usize, UpstrokeError> {`

Reclaim every durable gate-worktree intent in `store`. Callers must use
the same repository that created the store; intent names contain no
path supplied by the candidate and cannot escape these fixed children.

## `impl Workspace` › `let workspace = Workspace {`

The exact path is already known to be the new worktree's top level.
Avoid round-tripping it through Git's textual path output, which is
not necessarily UTF-8 on Unix.

## `impl Workspace` › `pub fn prepare_commit_from_candidate(`

Prepare and pin a commit from the exact candidate identities already
used by gates and review. This never rereads the mutable index.

## `impl Workspace` › `pub fn commit(&self, message: &str) -> Result<String, UpstrokeError> {`

Commit whatever `capture_diff` staged. §14: commit-per-task,
`[upstroke] <task-id>: <title>`.

## `impl Workspace` › `fn symbolic_ref_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {`

Return the immediate symbolic target without dereferencing it.

## `impl Workspace` › `pub fn remove_orphan_prepared_pin(&self, pin_ref: &str) -> Result<(), UpstrokeError> {`

Remove a private pin for an attempt that never durably recorded a
successful settlement. The target is read then supplied as the expected
old value, so even cleanup is compare-and-swap.

## `impl Workspace` › `pub fn discard_uncommitted(&self) -> Result<(), UpstrokeError> {`

Discard everything since the last commit: staged, unstaged, and
untracked (ignored files survive). This is both the §14 rollback on a
failed attempt and the post-commit scrub that keeps gate side-effects
(build artifacts, lockfile churn) from leaking into the next task's
captured diff.

## `enum SnapshotStoreMode` › `EphemeralUnderRoot,`

`store_or_root` is a shared parent such as the system temp directory.
Create one atomically private child and remove it after normal cleanup.

## `enum SnapshotStoreMode` › `ExactDurable,`

`store_or_root` is the stable per-run store whose intents resume scans.

## `impl PendingGateWorkspace` › `fn create(source_root: &Path, temp_root: &Path) -> Result<Self, UpstrokeError> {`

Create a uniquely named, owner-private store beneath a caller-owned
root. The root may be shared (notably `/tmp`) and is never chmodded.

## `impl PendingGateWorkspace` › `fn create_in_store(source_root: &Path, store: &Path) -> Result<Self, UpstrokeError> {`

Use the exact stable store whose synced intents resume will reclaim.

## `let _ = fs::remove_dir_all(path);`

`worktree remove` normally removes the directory too. Once Git confirms
no registration remains, these exact private paths are safe to remove
even if a partially failed add populated only part of either one.

## `fn sparse_checkout_is_refused_before_worker_spend()` › `run_git(&repo, &["update-index", "--skip-worktree", "README.md"]);`

Set these in separate commands: update-index applies only the final
mode option from one invocation. The combination guards against a
detector accidentally keying off assume-unchanged's presentation.

## `fn clean_detection_and_rollback()` › `let readme = fs::read_to_string(repo.join("README.md")).expect("read");`

core.autocrlf may legitimately restore CRLF on Windows checkouts.

## `fn branch_diff_commit_cycle()` › `let full = ws.head_sha_full().expect("full sha");`

What `resume` reads to recognise a commit as its own.

## `fn captured_candidate_keeps_one_parent_tree_and_diff()` › `fs::write(repo.join("README.md"), "second candidate\n").expect("second edit");`

Advance the index after capture, then prove the supplied tree still
materializes the first candidate rather than rereading that index.

## `fn open_normalizes_to_the_worktree_toplevel()` › `let expected = fs::canonicalize(&repo).expect("canonical repo");`

Compare canonically: temp dirs may be reached via a symlinked path.

## `fn capture_diff_is_immune_to_user_diff_config()` › `let set = |k: &str, v: &str| {`

Simulate a user with difftastic-style config and forced color.

## `fn opaque_git_diffs_are_rejected_before_review()` › `fs::write(repo.join(".gitattributes"), "hidden.rs -diff\n").expect("attributes");`

A candidate controls .gitattributes. Without --binary, marking a
source path -diff replaces all changed bytes with the tiny sentence
"Binary files differ", which a read-only reviewer cannot recover.

## `fn filtered_paths_are_refused_before_gates_and_review()` › `run_git(&repo, &["add", "-A"]);`

Preserve the independent post-stage guard for a caller opening an
index prepared outside Workspace::capture_candidate.

## `fn filter_on_unchanged_tracked_path_is_refused_before_materialization() {` › `fs::write(`

Only the attributes file changes. The filter target itself is absent
from `diff --cached --name-only` but is still a gate input.

## `fn capture_candidate_refuses_filter_before_candidate_helper_executes() {` › `run_git(&repo, &["add", "-A"]);`

Control: the exact raw command that capture used to run executes the
fixture, proving marker absence above is suppression rather than a
helper that could never run on this platform.

## `fn failed_gate_snapshot_add_cleans_registered_worktree()` › `ws.add_gate_worktree(path, hooks_path, commit)?;`

Model the dangerous failure boundary: Git has registered and
populated the worktree, then the overall add operation is
reported as failed (as a failing post-checkout hook did).

## `fn gate_snapshot_owner_helper()` › `let root = snapshot.workspace().root().to_path_buf();`

The snapshot's *identifier*, not its path. CODING_STANDARDS.md §12:
"a path is not safely a line: an ancestor may contain the delimiter,
or bytes that are not text at all. Send an identifier the receiver can
rejoin to a root it already knows." The parent supplied `store`, and
`create_in_store_inner` puts every snapshot at `<store>/worktrees/
<name>`, so the name is all the parent is missing -- and unlike the
path it is `upstroke-gates-<pid>-<ulid>`, which no ancestor can spoil.

## `fn gate_snapshot_owner_helper()` › `readiness::publish(&ready, &[name]).expect("publish the snapshot identity");`

Published last, and atomically. This used to be `fs::write` straight
to `ready`, which creates the name and then fills it -- so the parent,
which polls for the path and then reads it, could read nothing.

## `fn hard_killed_snapshot_owner_is_reclaimed_before_resume()` › `let mut owner = readiness::Producer::adopt(`

Adopted, so a panicking assertion anywhere below still terminates and
reaps this child rather than leaving it to sleep out its thirty
seconds holding a registered worktree.

## `fn hard_killed_snapshot_owner_is_reclaimed_before_resume()` › `let published = readiness::await_signal(&ready, owner.child(), Duration::from_secs(15))`

Producer-aware, and the bound is the one this test already used. The
wait it replaces polled only for the path, so an owner that died
before publishing -- a failed `Workspace::open`, a store the helper
could not create -- was reported fifteen seconds later as a producer
that had never published, which is the clock talking rather than the
death (CODING_STANDARDS.md §12).

## `fn hard_killed_snapshot_owner_is_reclaimed_before_resume()` › `let snapshot_path = store.join("worktrees").join(name);`

Rejoined to the root the parent already knew.
