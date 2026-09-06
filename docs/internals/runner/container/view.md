# `src/runner/container/view.rs`

Extended notes for [`src/runner/container/view.rs`](../../../../src/runner/container/view.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/runner/container/view.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

R19 — the disposable role-scoped Git view.

DESIGN.md:612, and every clause in it is an independently droppable
property:

> Because a linked worktree's `.git` points back into the real repository,
> the container overlays a disposable role-scoped Git view — **exact
> detached HEAD/index**, **no engine refs**, **read-only objects** — so
> Git-dependent tools work without exposing or mutating the coordinator's
> refs.

[`super::DisposableDirView`] is the directory half of the row, and it is
what the substrate's own tests use. This is the projection: what is *in* the
directory, and why each thing is there.

#### The four properties, and the mechanism for each

| property | mechanism | what a container would otherwise see |
|---|---|---|
| exact detached HEAD | `HEAD` holds the resolved commit id, never `ref: …` | the coordinator's branch name, and a checkout that moves when the coordinator moves it |
| exact index | the worktree's `index` is copied in, byte for byte | an empty index, so `git status` reports the whole tree as added |
| no engine refs | `refs/heads` and `refs/tags` are created empty and no `packed-refs` is written | `refs/upstroke/**` — every candidate, pin and integration ref of every run |
| read-only objects | `objects/info/alternates` names the object store, which Git **borrows and never writes to**, and the runner mounts that store `:ro` besides | a writable object store shared with the coordinator |
| disposable | the whole directory is [`GitView::discard`]ed at release, and every object Git writes lands in the view's own `objects/` | mutations in the coordinator's repository |

The alternate and the `:ro` mount are **both** used, and that is the point
rather than belt-and-braces. A `:ro` bind of the object store *alone* would
make every write-side Git operation fail hard — `git add`, `git stash`,
`git write-tree`, which repository-controlled gates really do run. An
alternate *alone* would leave the coordinator's store writable through the
mount. Together, reads resolve through a store the kernel will not let the
container write, and writes land in the view's own disposable half.

#### What is deliberately **not** here

No `commondir`, no `gitdir`, no `worktrees/`, no `config` section naming a
remote, a URL or a credential helper. Those are the links back into the real
repository the sentence above exists to cut. The census
[`tests::the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository`]
asserts their absence by name rather than trusting this list.

## `#![allow(clippy::disallowed_methods)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`. This
module is the body of `GitView::materialize`/`discard`, whose two methods are
themselves on the denylist as `Container.MountGitView`/`Container.
UnmountGitView`; the effects it performs are the R19 directory and its
contents, and it returns a `PathBuf`, never a writable handle. The same
placement `src/events/log.rs` has: the funnel's declaration is in the module
the packet names and the body is beside it.
`decisions.effect_site_inventory.mechanism` (2).

## `#![deny(clippy::disallowed_types, clippy::disallowed_macros)]`

`PR6-LANEF-004`: the two lints this file does NOT allow are re-denied here,
because the Container funnel's allow is an inner attribute and would
otherwise reach this module through the module tree. The `allow` above is
this file's own, reviewed in `effects/allowlist.toml`; these two are not.

## `const GITDIR_PREFIX: &str = "gitdir:";`

The file a linked worktree carries instead of a `.git` directory.

## `const COMMONDIR: &str = "commondir";`

What Git calls the file that points a worktree's Git directory at the
repository's shared half.

## `const DOT_GIT: &str = ".git";`

The name of a Git directory inside a worktree.

## `const OBJECTS: &str = "objects";`

Where a Git directory keeps its objects.

## `const ALTERNATES: &str = "objects/info/alternates";`

Git's own read-only borrow of another object store.

`objects/info/alternates`, one absolute path per line. Git resolves objects
through it and **never writes to it**, which is the property DESIGN.md:612
asks for in the words "read-only objects".

## `pub const WORKTREE_GITFILE: &str = "worktree.gitfile";`

The one-line file that is mounted at `<workspace>/.git`.

Exactly what a linked worktree's own `.git` is — `gitdir: <path>` — so the
overlay is the shape Git already understands rather than an environment
variable a tool could ignore. It lives *inside* the view so that the whole
of R19 is one directory with one `discard`; Git ignores entries it does not
know in a Git directory.

## `pub const SHARED_INDEX_PREFIX: &str = "sharedindex.";`

The prefix of a split index's shared half, which lives beside `index` in the
**worktree's own** Git directory.

`git update-index --split-index` (and `core.splitIndex = true`, which
`feature.manyFiles` turns on) writes most of the index's entries into
`<git-dir>/sharedindex.<oid>` and leaves `index` holding a `link` extension
naming it. An `index` copied without it is an index Git refuses to read at
all — measured, verbatim:

```text
fatal: <view>/sharedindex.<oid>: index file open failed: No such file or directory
```

so `PR6-CORRECTNESS-010`: DESIGN.md:612's "**exact** detached HEAD/index …
so Git-dependent tools work" was false of every split-index worktree, and
the gate that reads it would fail on the repository's Git configuration
rather than on its own subject. It is index data of the same repository, so
it carries no ref, no remote and no credential helper — nothing
[`WITHHELD_ENTRIES`] is about.

## `pub struct GitLayout {`

---------------------------------------------------------------------------
Where a worktree's Git actually is
---------------------------------------------------------------------------

## `pub struct GitLayout {`

The three Git directories a worktree has, told apart.

A linked worktree has all three at different places, which is the whole
reason this module exists: `<workspace>/.git` is a *file*, the per-worktree
Git directory holds `HEAD` and `index`, and the objects live in the
repository's shared half. A view built from the wrong one of the three is a
view of the wrong thing.

## `pub struct GitLayout` › `pub git_dir: PathBuf,`

The worktree's own Git directory: `HEAD`, `index`.

## `pub struct GitLayout` › `pub common_dir: PathBuf,`

The repository's shared Git directory: `objects`, `packed-refs`,
`config`, and **every engine ref**.

## `pub struct GitLayout` › `pub objects: PathBuf,`

`<common_dir>/objects`.

## `pub struct GitLayout` › `pub dot_git_is_file: bool,`

Whether `<workspace>/.git` is a **file** — the linked-worktree shape.

The mount plan needs it: a directory cannot be bind-mounted over a file
and a file cannot be bind-mounted over a directory, so which of the two
the view is overlaid with follows from this. Measured, not assumed —
see [`super::env::BoundaryLayout::DEFAULT_GIT_VIEW`].

## `pub fn resolve(workspace: &Path) -> Result<Option<GitLayout>, UpstrokeError> {`

Where `workspace`'s Git is, or `None` when it has none.

`None` is a real answer and not a failure: R19's granularity is "per
container invocation (**incl. shell and agent probes**)", and a probe's
workspace is a scratch directory with no repository in it. Such an
invocation still gets a view directory — the row is per invocation — and the
view is simply empty.

### Errors

[`UpstrokeError::Io`] when `<workspace>/.git` exists and cannot be read, or
[`UpstrokeError::Git`] when it is a `gitdir:` file naming nothing.

## `pub fn resolve(workspace: &Path) -> Result<Option<GitLayout…` › `let common_dir = match fs::read_to_string(git_dir.join(COMMONDIR)) {`

`commondir` is what a linked worktree carries to name the repository's
shared half. A main worktree has none, and is its own common dir.

## `fn normalized(path: &Path) -> PathBuf {`

`path` with `.` and `..` components resolved **lexically**.

A linked worktree's `commondir` is `../..`, so the joined path is
`<repo>/.git/worktrees/<name>/../..`. That names the right directory to
every filesystem call and the *wrong* one to every lexical operation — and
two lexical operations depend on it: this value is written into
`objects/info/alternates`, which a reader inside a container resolves as
text, and [`super::exec::Confinement`] compares mount sources against
withheld paths with `Path::starts_with`, which is a component-wise prefix
test and not a filesystem one.

Deliberately **not** `fs::canonicalize`: on Windows that returns a
`\\?\`-prefixed verbatim path, which several tools read as a literal
directory name, and this crate targets Windows first-class. The cost is that
a symbolic link on the chain is not resolved here; the chain-validation that
refuses reparse points is `workspace_manager::validate_execution_root_chain`
and it runs before a worktree exists.

## `pub fn detached_head(layout: &GitLayout) -> Result<String, UpstrokeError> {`

The **exact** commit this worktree is at, as an object id.

"exact **detached** HEAD": a symbolic `ref: refs/heads/…` is resolved here,
on the coordinator, so the view carries an id rather than a name. A view
carrying the name would need the ref to exist inside it, and the refs are
exactly what the view withholds.

### Errors

[`UpstrokeError::Git`] when `HEAD` is missing, names a ref nothing resolves,
or does not resolve to an object id.

## `pub fn detached_head(layout: &GitLayout) -> Result<String, …` › `for base in [&layout.git_dir, &layout.common_dir] {`

A loose ref, in the worktree's own half first (`refs/bisect/**` and
`HEAD` are per-worktree) and then in the shared half.

## `pub fn detached_head(layout: &GitLayout) -> Result<String, …` › `let packed_path = layout.common_dir.join("packed-refs");`

Then `packed-refs`, whose lines are `<id> <name>`.

## `fn object_id(value: &str, from: &Path) -> Result<String, UpstrokeError> {`

A value that is an object id, or a refusal naming where it came from.

Forty characters for `sha1` and sixty-four for `sha256`, because
[`config_for`] carries the repository's `[extensions]` across and a
`sha256` repository is a thing this view has to be able to project.

## `pub struct RoleGitView {`

---------------------------------------------------------------------------
The projection
---------------------------------------------------------------------------

## `pub struct RoleGitView {`

The R19 disposable role-scoped Git view.

Implements [`GitView`], so the funnel — [`super::mount_git_view`] and
[`super::unmount_git_view`], the two `Container.MountGitView` /
`Container.UnmountGitView` APIs — is what a caller uses. Nothing here is
reachable except through those.

## `pub struct RoleGitView` › `reader: Option<ReaderPaths>,`

Where this view and the borrowed object store will be visible **to
whoever reads the view**.

`None` means "at the paths they are on this host", which is what a
coordinator-side reader needs. [`super::exec::ContainerRunner`] sets it
to the two **in-container** mount targets, because the reader is inside
the container and a `gitdir:` line or an alternate naming a host path
would name nothing there. That is the same class of defect as
`PR4-ADAPTER-RESOLVES-ON-THE-HOST`: a coordinator-host path serialized
into something a boundary with its own filesystem has to read.

One knob and not two, because the two files are read by the same reader
and a view whose `gitdir:` was in-container and whose alternate was not
would be half-projected — the shape nobody would notice until a gate ran.

## `pub struct ReaderPaths {`

Where the view and the borrowed object store are, as the reader sees them.

## `pub struct ReaderPaths` › `pub view: String,`

The view directory.

## `pub struct ReaderPaths` › `pub objects: String,`

The borrowed object store.

## `impl RoleGitView` › `pub fn new(trace: ContainerTrace) -> Self {`

A view whose actions are recorded in `trace`.

## `impl RoleGitView` › `pub fn for_reader(mut self, view: impl Into<String>, objects: impl Into<String>) -> Self {`

Project for a reader that will see the view at `view` and the borrowed
object store at `objects`.

## `impl RoleGitView` › `pub fn reader_paths(&self, request: &GitViewRequest, layout: &GitLayout) -> ReaderPaths {`

Where this view will tell a reader to look, given where it is on this
host.

## `pub const PROJECTED_ENTRIES: &[&str] = &[`

The files a projected view holds, in the order they are written.

Written out as a list so the census that proves the view carries nothing
else has something to compare against that is not the function that produced
it.

## `pub const WITHHELD_ENTRIES: &[&str] = &[COMMONDIR, "gitdir", "worktrees", "packed-refs"];`

The names a view must never carry, each being a link back into the real
repository.

`commondir` and `gitdir` are how a linked worktree finds the repository;
`worktrees` is the registry of every other one; `packed-refs` is where the
engine's refs live once Git has packed them. DESIGN.md:612's sentence is
about exactly these.

## `impl GitView for RoleGitView` › `fn discard(&self, path: &Path) -> Result<(), UpstrokeError> {`

The source retains the concurrent-removal protocol at this operation under
the concurrency exception in §§10 and 13. A successful call records
`Discarded` even when another reclaimer already removed the view.

**Through the one racing removal** (`PR6-CORRECTNESS-009`).

`crash_reconstruction` requires "every step idempotent and tolerant of
already-gone so **two concurrent reclaimers converge**", and on Windows
the loser of that race does not get `NotFound`: a directory whose last
handle has closed is *delete-pending*, and `remove_dir_all` reports
`PermissionDenied` until the name actually goes away. This implementation
tolerated only `NotFound`, so one of two converging write commands
refused. [`super::DisposableDirView`] had already been repaired; this
one is the projection a real run mounts, and the concurrent-census
fixtures all used the other.

**Tolerating `PermissionDenied` outright would be the wrong repair** and
it is worth saying why, because it is the smaller diff: a view that is
genuinely protected — an open handle inside it on Windows, a read-only
parent on Unix — would then be reported discarded, the census would go on
to remove the intent, and admission would proceed over R19 residue that
nothing will ever reclaim, because the record naming it is gone.
`super::racing_removal` retries and then fails if every attempt still reports
an error. It gives a delete-pending name another chance to disappear without
treating access denial as success. The bound used to be sixty-four yields,
which a winner descheduled between marking the name and closing its handle
outlasted; [reviews/FINDINGS.md §43](https://github.com/eventloops/upstroke/blob/master/reviews/FINDINGS.md) records
the first Windows sighting and `PR154-WINDOWS-CENSUS-VIEW-REMOVAL-ACCESS-DENIED`
the measured cause. The later attempts now sleep, on the schedule
`super::racing_pause` and its constants state. A protected view remains an
error.

## `fn project(`

Write the projection into `view`.

## `write_file(&view.join("HEAD"), format!("{head}\n").as_bytes())?;`

Exact detached HEAD: an id, never a name.

## `write_file(&view.join("config"), config_for(layout)?.as_bytes())?;`

The repository format, and any extension the object store depends on.

## `match fs::read(layout.git_dir.join("index")) {`

Exact index. A worktree with no index yet is a real state — nothing has
been staged — and an absent index is what Git expects then, not an empty
file, which Git reads as a corrupt one.

## `copy_shared_indexes(&layout.git_dir, view)?;`

And the half a **split** index keeps beside it. See
[`SHARED_INDEX_PREFIX`]: without this the projected index is one Git
refuses to open, so "exact index" holds only for repositories that do not
use `core.splitIndex`.

## `create_dir(&view.join("objects").join("info"))?;`

Read-only objects: borrowed through Git's own alternate mechanism, which
Git resolves through and never writes to. Every object this view's reader
creates lands in `objects/` below, which the release prunes.

## `create_dir(&view.join("refs").join("heads"))?;`

No engine refs: the two directories Git requires, both empty, and no
`packed-refs`.

## `write_file(`

The overlay itself: what `<workspace>/.git` becomes.

## `fn copy_shared_indexes(git_dir: &Path, view: &Path) -> Result<Vec<String>, UpstrokeError> {`

Copy every `sharedindex.*` beside the source index into the view, and answer
which names were copied.

**Every one, not the one the `link` extension names.** Reading that name out
of the index means parsing the index: the extension block sits after the
entries, index v4 prefix-compresses entry paths, and a scan for the four
bytes `link` matches a path that happens to contain them. A wrong parse here
silently drops the file again, which is the failure being repaired. Git
keeps at most a small number of these — the live one and, briefly, the one
it replaced — they are index data of the repository the view already
borrows, and the whole directory is discarded with the invocation.

### Errors

[`UpstrokeError::Io`] when the Git directory or a shared index cannot be read.

## `fn copy_shared_indexes(git_dir: &Path, view: &Path) -> Resu…` › `Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,`

Git replaced it between the listing and the read; the `index`
this view carries names the one that is there, and a shared index
that went away under the scan was not it.

## `fn config_for(layout: &GitLayout) -> Result<String, UpstrokeError> {`

The view's `config`.

Minimal by construction rather than copied: the repository's own config
carries remotes, URLs and credential helpers, and a view that copied it
would hand a container the operator's forge credentials — the opposite of
what R19 is for. What *is* carried over is the repository format and the
`[extensions]` section, because those describe the object store the view
borrows: a `sha256` repository read as `sha1` is a repository read wrong,
and an unknown extension declared is a Git that refuses loudly instead of
one that misreads.

## `pub(crate) mod fixtures {`

-- test-only declarations ----------------------------------------------
At the BOTTOM: `effects::production_region`, which
`effects::externally_reachable_fns` and the three censuses in `super::exec`
still use, cuts a source at its first `#[cfg(test)]`
(`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

## `pub(crate) mod fixtures {`

Real temporary Git repositories, built through the Runner.

`decisions.tests_acceptance.determinism` says "**real temporary Git
repositories**", and this module is the one that builds them for the
container lane. Every `git` here goes through
[`crate::runner::host::HostRunner`] rather than through a
`std::process::Command` of its own — which is the same rule the production
tree obeys ("every CLI and gate process executes through Runner") and, in
passing, keeps `std::process::Command` out of this module's lint set.

`pub(crate)` and declared here rather than inside `mod tests`, because
`super::exec`'s suite builds the same repositories and two copies of a
fixture are two fixtures that drift.

**A `pub(crate) mod` used to read as production to the source censuses in
`src/runner/mod.rs`.** Their region was `runner::tests::production_region`,
whose predicate was that the line after the cfg attribute starts with a bare
`mod` keyword; a visibility qualifier in front of it defeated that, and this
block was scanned as production. **That reader is deleted.** All four
whole-tree censuses now share `effects::production_code`, which finds the
item's extent by delimiter matching rather than by reading a line, so
`#[cfg(test)] pub(crate) mod fixtures { … }` is removed like any other
configured item — measured on this file, which contributes nothing to any of
them.

Nothing here constructs a process, a spawn, a timed run, a role literal or a
request literal anyway, and every `git` goes through the gate-request builder
and the host runner. That is now a discipline rather than a requirement, and
it is kept: it is what makes the block safe to read as production if a future
region ever does.

**All four censuses blank comments *and* string literals now**
(`PR5-R1-PROCESS-START-CENSUS-UNSTRIPPED`, closed), so a doc comment here
that names one of their needles no longer changes an expected count. It once
did: the paragraph above, in its first spelling, added a phantom row for this
file to two of them — the sixth occurrence of `PR4-CENSUS-COMMENT-ORACLE` on
this project, and the one that finally moved the blanking into the shared
region instead of into each census.

## `pub(crate) mod fixtures` › `pub(crate) fn scratch(tag: &str) -> PathBuf {`

A scratch directory, in the idiom of `runner::container::tests::scratch`.

## `pub(crate) mod fixtures` › `pub(crate) fn git(cwd: &Path, args: &[&str]) -> ProcessOutput {`

One `git` invocation, in `cwd`, through the host runner.

## `pub(crate) fn git(cwd: &Path, args: &[&str]) -> ProcessOutp…` › `for fixed in [`

A fixed identity, so a commit is a function of its inputs rather than
of whoever's `~/.gitconfig` the suite runs under — and so a machine
with no identity configured can still build a fixture.

## `pub(crate) mod fixtures` › `pub(crate) fn git_ok(cwd: &Path, args: &[&str]) -> String {`

One `git` invocation that must succeed, with its trimmed stdout.

## `pub(crate) mod fixtures` › `pub(crate) fn repository(dir: &Path) -> (String, String) {`

A repository with two commits, at `dir`.

Returns `(head, previous)` — two distinct object ids, so a test can move
a worktree between them and see the view follow.

## `pub(crate) mod fixtures` › `pub(crate) fn worktree(repo: &Path, at: &Path, commit: &str) {`

A detached linked worktree of `repo` at `at`, checked out at `commit`.

## `pub(crate) mod fixtures` › `pub(crate) fn engine_refs(repo: &Path, commit: &str) -> Vec<String> {`

Engine refs, of the shape `src/workspace_manager.rs` writes.

## `mod tests` › `fn a_linked_worktrees_three_git_directories_resolve_to_three_distinct_places() {`

A linked worktree has three Git directories at three places, and a main
worktree has two of the three at one.

Second field held constant: the repository — the *same* repository is
resolved twice — so what varies is only which worktree is asked.

## `fn a_linked_worktrees_three_git_directories_resolve_to_thre…` › `let places: std::collections::BTreeSet<PathBuf> = [`

Three distinct places, counted rather than described.

## `mod tests` › `fn a_workspace_with_no_repository_has_no_layout_and_still_gets_a_view() {`

A workspace with no repository is a real state, and it still gets a
view.

R19's granularity is "per container invocation (**incl. shell and agent
probes**)", and a probe's workspace is a scratch directory. A
`materialize` that refused there would make every probe unable to start.

## `mod tests` › `fn the_view_carries_the_exact_detached_head_and_index_of_the_worktree() {`

The view carries the worktree's **exact** detached HEAD and index.

The expected head comes from `git rev-parse` — Git's own answer, run by
the fixture — and never from [`detached_head`], which is the function
this pins. The second commit is the reason the fixture builds two: a
view whose HEAD was a constant, or was the repository's `HEAD` rather
than the worktree's, passes with one.

Second field held constant: the workspace path and the repository; what
varies is which commit the worktree is at.

## `fn the_view_carries_the_exact_detached_head_and_index_of_th…` › `let by_git = git_ok(&workspace, &["rev-parse", "HEAD"]);`

The oracle is git's, not ours.

## `fn the_view_carries_the_exact_detached_head_and_index_of_th…` › `let source_index = std::fs::read(layout.git_dir.join("index")).expect("the index");`

Exact index: the bytes, not a rebuild.

## `fn the_view_carries_the_exact_detached_head_and_index_of_th…` › `assert_ne!(`

The two views really do differ, so the assertions above are about the
worktree rather than about a constant.

## `mod tests` › `fn a_symbolic_head_is_resolved_to_an_object_id_before_it_reaches_the_view() {`

A **symbolic** HEAD is resolved to an object id before it reaches the
view.

"exact **detached** HEAD". A view carrying `ref: refs/heads/main` would
need that ref to exist inside it, and the refs are exactly what the view
withholds — so a tool reading such a view sees an unborn branch.

## `fn a_symbolic_head_is_resolved_to_an_object_id_before_it_re…` › `let raw = std::fs::read_to_string(repo.join(".git").join("HEAD")).expect("HEAD");`

The main worktree's HEAD *is* symbolic.

## `fn a_symbolic_head_is_resolved_to_an_object_id_before_it_re…` › `git_ok(&repo, &["pack-refs", "--all"]);`

And through packed-refs, which is where the loose ref goes when Git
packs it — the other half of the resolution and a separate branch of
the code.

## `mod tests` › `fn a_head_that_names_nothing_refuses() {`

A HEAD that is not an object id refuses rather than producing a view of
nothing.

## `fn a_head_that_names_nothing_refuses()` › `std::fs::write(layout.git_dir.join("HEAD"), "not-an-object-id\n").expect("plant");`

And a HEAD holding something that is not an id at all.

## `mod tests` › `fn the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository() {`

No engine refs, and no link back into the real repository.

The repository is loaded with the three shapes of engine ref
`src/workspace_manager.rs` writes — a candidate, an integration ref and
a prepared pin — so the count on the repository side is the control: a
view that carried them would differ from zero, and a fixture that
planted none would not.

Second field held constant: the worktree and its HEAD; what varies is
which repository the refs are read from.

## `fn the_role_view_carries_no_engine_refs_and_no_link_back_in…` › `git_ok(&repo, &["pack-refs", "--all"]);`

Pack them, so the view cannot avoid them merely by not copying loose
files.

## `fn the_role_view_carries_no_engine_refs_and_no_link_back_in…` › `assert_eq!(WITHHELD_ENTRIES.len(), 4);`

Every name that would link back, by name rather than by inspection.

## `fn the_role_view_carries_no_engine_refs_and_no_link_back_in…` › `for dir in ["refs/heads", "refs/tags"] {`

The refs the view does carry: two empty directories and nothing else.

## `fn the_role_view_carries_no_engine_refs_and_no_link_back_in…` › `let config = std::fs::read_to_string(view_path.join("config")).expect("config");`

And the config names no remote, no URL and no credential helper: a
view that copied the repository's config would hand a container the
operator's forge credentials.

## `fn the_role_view_carries_no_engine_refs_and_no_link_back_in…` › `for entry in PROJECTED_ENTRIES {`

The projection is exactly the entries the module declares.

## `mod tests` › `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_the_engines_refs() {`

`proof_tests[1]`: a Git-dependent tool sees only the role view.

Real Git, over the real projection, on the host — so this holds on every
platform the suite runs on, with no container runtime. The container
half is `exec::tests::real_docker_a_git_dependent_gate_sees_only_the_role_view`,
which runs the same commands inside a container over the same view.

Second field held constant: the commands are the *same* commands run
against the worktree's own Git directory first — that is the control
pair, so "the view answers" and "the view withholds" are both measured
against a run that is known to do the opposite.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `assert_eq!(`

Objects resolve — through the alternate, which is Git's own
read-only borrow.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `assert_eq!(`

The index is exact, so a clean worktree reads clean.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `assert_eq!(`

The Git directory the tool reports is the view, not the coordinator's.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `assert_eq!(`

And no engine ref is visible.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `assert_eq!(`

The control: the *same* command against the worktree's own Git
directory does resolve it, so the assertion above is about the
view rather than about the command.

## `fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_…` › `let after = git_ok(&repo, &["for-each-ref", "--format=%(refname)"]);`

The coordinator's refs are unchanged by any of it.

## `mod tests` › `fn an_object_written_through_the_view_lands_in_the_view_and_not_in_the_repository() {`

A write through the view lands in the view, never in the repository.

"**read-only objects** … without exposing or **mutating** the
coordinator's refs". The alternate is what Git reads through and never
writes to, so an object created against the view goes into the view's
own `objects/` — which the release prunes.

## `fn an_object_written_through_the_view_lands_in_the_view_and…` › `assert!(written.is_empty());`

`--stdin-paths` with no stdin writes nothing; use a real file instead.

## `fn an_object_written_through_the_view_lands_in_the_view_and…` › `for round in 0..2 {`

Discarded, twice: idempotent, and nothing is left.

## `fn an_object_written_through_the_view_lands_in_the_view_and…` › `assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head);`

The repository is untouched by the discard.

## `mod tests` › `fn count_objects(objects: &Path) -> usize {`

Loose objects under `objects/`, ignoring `info` and `pack` metadata.

## `mod tests` › `fn the_projection_names_the_paths_the_reader_will_see_and_not_the_hosts() {`

The `gitdir:` line and the alternate name the paths **the reader** will
see, not the ones the coordinator sees.

A coordinator-host path written into a file a container reads is
`PR4-ADAPTER-RESOLVES-ON-THE-HOST`'s shape, one layer down: it names
nothing inside the image. Both files are checked, because a view whose
`gitdir:` was in-container and whose alternate was not would be
half-projected — a Git that finds the view and then cannot find an
object, which reads as a corrupt repository rather than as a mistake
here.

Second field held constant: the workspace and its layout; what varies is
who the reader is.

## `fn the_projection_names_the_paths_the_reader_will_see_and_n…` › `let host_view = root.join("view-host");`

The coordinator's reader: the host paths.

## `fn the_projection_names_the_paths_the_reader_will_see_and_n…` › `let container_view = root.join("view-container");`

A container's reader: the in-container mount targets.

## `mod tests` › `fn the_dot_git_kind_is_read_from_the_worktree_and_takes_both_values() {`

The `.git` kind is read from the worktree, and it decides the mount
shape.

Measured against `docker` 29.7.2: a directory cannot be bind-mounted
onto a file. A `GitLayout` that always reported one kind would produce a
container that fails at `runc create` for the other — a failure with no
test above it, because every fixture in this file would have used the
kind that happened to work.

Second field held constant: the repository; what varies is which of its
worktrees is asked.

## `fn the_dot_git_kind_is_read_from_the_worktree_and_takes_bot…` › `let kinds: std::collections::BTreeSet<bool> =`

Both values are taken, which is what makes the mount-shape match in
`exec::ContainerRunner::mounts` reachable on both arms.

## `mod split_index_tests {`

---------------------------------------------------------------------------
R3b: the half a split index keeps beside itself
---------------------------------------------------------------------------

## `mod split_index_tests` › `fn a_split_index_projects_with_the_shared_half_it_links_to() {`

A **split** index projects whole, and real Git reads it.

`PR6-CORRECTNESS-010`. `core.splitIndex` — which
`git update-index --split-index` sets and which `feature.manyFiles`
turns on — moves most of the index's entries into
`<git-dir>/sharedindex.<oid>` and leaves `index` holding a `link`
extension naming it. The projection copied `index` alone, so the view's
index named a file that was not there and Git refused to open it at all:

```text
fatal: <view>/sharedindex.<oid>: index file open failed: No such file or directory
```

DESIGN.md:612 is "**exact** detached HEAD/index … so Git-dependent tools
work", and a gate reading that view failed on the repository's Git
configuration rather than on its own subject. No fixture created a split
index, so the whole class was invisible.

The grid is **{ordinary index, split index} × {is the view readable}**,
and the ordinary cell is the control: it carries no `sharedindex.*`, so
"the view works" in the split cell is attributable to the copy and not
to Git ignoring the extension.

The oracle is **Git**, not this module: each cell runs
`git ls-files` against the projected view and compares it with the same
command against the source worktree. A projection that produced an index
this code was happy with and Git was not would fail here.

Second field held constant: the same repository, the same worktree, the
same staged file and the same HEAD in both cells; only whether the index
was split moves.

## `fn a_split_index_projects_with_the_shared_half_it_links_to()` › `std::fs::write(workspace.join("staged.txt"), "staged\n").expect("a file");`

Something staged, so the index is not the empty one and
`ls-files` has an answer that can differ.

## `fn a_split_index_projects_with_the_shared_half_it_links_to()` › `assert_eq!(`

The premise of each cell, measured rather than assumed: only the
split one has a shared half.

## `fn a_split_index_projects_with_the_shared_half_it_links_to()` › `for name in &shared {`

Every shared half the source had, the view has — by name.

## `fn a_split_index_projects_with_the_shared_half_it_links_to()` › `let through_view = crate::runner::container::view::fixtures::git(`

The oracle: Git itself, over the projected view, compared with
Git over the source worktree.
