//! R19 — the disposable role-scoped Git view.
//!
//! DESIGN.md:612, and every clause in it is an independently droppable
//! property:
//!
//! > Because a linked worktree's `.git` points back into the real repository,
//! > the container overlays a disposable role-scoped Git view — **exact
//! > detached HEAD/index**, **no engine refs**, **read-only objects** — so
//! > Git-dependent tools work without exposing or mutating the coordinator's
//! > refs.
//!
//! [`super::DisposableDirView`] is the directory half of the row, and it is
//! what the substrate's own tests use. This is the projection: what is *in* the
//! directory, and why each thing is there.
//!
//! ## The four properties, and the mechanism for each
//!
//! | property | mechanism | what a container would otherwise see |
//! |---|---|---|
//! | exact detached HEAD | `HEAD` holds the resolved commit id, never `ref: …` | the coordinator's branch name, and a checkout that moves when the coordinator moves it |
//! | exact index | the worktree's `index` is copied in, byte for byte | an empty index, so `git status` reports the whole tree as added |
//! | no engine refs | `refs/heads` and `refs/tags` are created empty and no `packed-refs` is written | `refs/upstroke/**` — every candidate, pin and integration ref of every run |
//! | read-only objects | `objects/info/alternates` names the object store, which Git **borrows and never writes to**, and the runner mounts that store `:ro` besides | a writable object store shared with the coordinator |
//! | disposable | the whole directory is [`GitView::discard`]ed at release, and every object Git writes lands in the view's own `objects/` | mutations in the coordinator's repository |
//!
//! The alternate and the `:ro` mount are **both** used, and that is the point
//! rather than belt-and-braces. A `:ro` bind of the object store *alone* would
//! make every write-side Git operation fail hard — `git add`, `git stash`,
//! `git write-tree`, which repository-controlled gates really do run. An
//! alternate *alone* would leave the coordinator's store writable through the
//! mount. Together, reads resolve through a store the kernel will not let the
//! container write, and writes land in the view's own disposable half.
//!
//! ## What is deliberately **not** here
//!
//! No `commondir`, no `gitdir`, no `worktrees/`, no `config` section naming a
//! remote, a URL or a credential helper. Those are the links back into the real
//! repository the sentence above exists to cut. The census
//! [`super::exec::tests::the_role_view_carries_no_link_back_into_the_repository`]
//! asserts their absence by name rather than trusting this list.
// Allowlist placement: the **funnel section** of `effects/allowlist.toml`. This
// module is the body of `GitView::materialize`/`discard`, whose two methods are
// themselves on the denylist as `Container.MountGitView`/`Container.
// UnmountGitView`; the effects it performs are the R19 directory and its
// contents, and it returns a `PathBuf`, never a writable handle. The same
// placement `src/events/log.rs` has: the funnel's declaration is in the module
// the packet names and the body is beside it.
// `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]
// `PR6-LANEF-004`: the two lints this file does NOT allow are re-denied here,
// because the Container funnel's allow is an inner attribute and would
// otherwise reach this module through the module tree. The `allow` above is
// this file's own, reviewed in `effects/allowlist.toml`; these two are not.
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;

use super::runtime::{ContainerTrace, ViewAction};
use super::{GitView, GitViewRequest};

/// The file a linked worktree carries instead of a `.git` directory.
const GITDIR_PREFIX: &str = "gitdir:";

/// What Git calls the file that points a worktree's Git directory at the
/// repository's shared half.
const COMMONDIR: &str = "commondir";

/// The name of a Git directory inside a worktree.
const DOT_GIT: &str = ".git";

/// Where a Git directory keeps its objects.
const OBJECTS: &str = "objects";

/// Git's own read-only borrow of another object store.
///
/// `objects/info/alternates`, one absolute path per line. Git resolves objects
/// through it and **never writes to it**, which is the property DESIGN.md:612
/// asks for in the words "read-only objects".
const ALTERNATES: &str = "objects/info/alternates";

/// The one-line file that is mounted at `<workspace>/.git`.
///
/// Exactly what a linked worktree's own `.git` is — `gitdir: <path>` — so the
/// overlay is the shape Git already understands rather than an environment
/// variable a tool could ignore. It lives *inside* the view so that the whole
/// of R19 is one directory with one `discard`; Git ignores entries it does not
/// know in a Git directory.
pub const WORKTREE_GITFILE: &str = "worktree.gitfile";

/// The prefix of a split index's shared half, which lives beside `index` in the
/// **worktree's own** Git directory.
///
/// `git update-index --split-index` (and `core.splitIndex = true`, which
/// `feature.manyFiles` turns on) writes most of the index's entries into
/// `<git-dir>/sharedindex.<oid>` and leaves `index` holding a `link` extension
/// naming it. An `index` copied without it is an index Git refuses to read at
/// all — measured, verbatim:
///
/// ```text
/// fatal: <view>/sharedindex.<oid>: index file open failed: No such file or directory
/// ```
///
/// so `PR6-CORRECTNESS-010`: DESIGN.md:612's "**exact** detached HEAD/index …
/// so Git-dependent tools work" was false of every split-index worktree, and
/// the gate that reads it would fail on the repository's Git configuration
/// rather than on its own subject. It is index data of the same repository, so
/// it carries no ref, no remote and no credential helper — nothing
/// [`WITHHELD_ENTRIES`] is about.
pub const SHARED_INDEX_PREFIX: &str = "sharedindex.";

// ---------------------------------------------------------------------------
// Where a worktree's Git actually is
// ---------------------------------------------------------------------------

/// The three Git directories a worktree has, told apart.
///
/// A linked worktree has all three at different places, which is the whole
/// reason this module exists: `<workspace>/.git` is a *file*, the per-worktree
/// Git directory holds `HEAD` and `index`, and the objects live in the
/// repository's shared half. A view built from the wrong one of the three is a
/// view of the wrong thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLayout {
    /// The worktree's own Git directory: `HEAD`, `index`.
    pub git_dir: PathBuf,
    /// The repository's shared Git directory: `objects`, `packed-refs`,
    /// `config`, and **every engine ref**.
    pub common_dir: PathBuf,
    /// `<common_dir>/objects`.
    pub objects: PathBuf,
    /// Whether `<workspace>/.git` is a **file** — the linked-worktree shape.
    ///
    /// The mount plan needs it: a directory cannot be bind-mounted over a file
    /// and a file cannot be bind-mounted over a directory, so which of the two
    /// the view is overlaid with follows from this. Measured, not assumed —
    /// see [`super::env::BoundaryLayout::DEFAULT_GIT_VIEW`].
    pub dot_git_is_file: bool,
}

/// Where `workspace`'s Git is, or `None` when it has none.
///
/// `None` is a real answer and not a failure: R19's granularity is "per
/// container invocation (**incl. shell and agent probes**)", and a probe's
/// workspace is a scratch directory with no repository in it. Such an
/// invocation still gets a view directory — the row is per invocation — and the
/// view is simply empty.
///
/// # Errors
///
/// [`UpstrokeError::Io`] when `<workspace>/.git` exists and cannot be read, or
/// [`UpstrokeError::Git`] when it is a `gitdir:` file naming nothing.
pub fn resolve(workspace: &Path) -> Result<Option<GitLayout>, UpstrokeError> {
    let dot_git = workspace.join(DOT_GIT);
    let metadata = match fs::symlink_metadata(&dot_git) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: dot_git,
                source,
            });
        }
    };

    let dot_git_is_file = !metadata.is_dir();
    let git_dir = if metadata.is_dir() {
        dot_git
    } else {
        let text = fs::read_to_string(&dot_git).map_err(|source| UpstrokeError::Io {
            path: dot_git.clone(),
            source,
        })?;
        let Some(target) = text.trim().strip_prefix(GITDIR_PREFIX) else {
            return Err(UpstrokeError::Git {
                message: format!(
                    "`{}` is neither a Git directory nor a `{GITDIR_PREFIX}` link",
                    dot_git.display()
                ),
            });
        };
        let target = Path::new(target.trim());
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            workspace.join(target)
        }
    };

    // `commondir` is what a linked worktree carries to name the repository's
    // shared half. A main worktree has none, and is its own common dir.
    let common_dir = match fs::read_to_string(git_dir.join(COMMONDIR)) {
        Ok(text) => {
            let target = Path::new(text.trim());
            if target.is_absolute() {
                target.to_path_buf()
            } else {
                git_dir.join(target)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => git_dir.clone(),
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: git_dir.join(COMMONDIR),
                source,
            });
        }
    };

    let git_dir = normalized(&git_dir);
    let common_dir = normalized(&common_dir);
    Ok(Some(GitLayout {
        objects: common_dir.join(OBJECTS),
        git_dir,
        common_dir,
        dot_git_is_file,
    }))
}

/// `path` with `.` and `..` components resolved **lexically**.
///
/// A linked worktree's `commondir` is `../..`, so the joined path is
/// `<repo>/.git/worktrees/<name>/../..`. That names the right directory to
/// every filesystem call and the *wrong* one to every lexical operation — and
/// two lexical operations depend on it: this value is written into
/// `objects/info/alternates`, which a reader inside a container resolves as
/// text, and [`super::exec::Confinement`] compares mount sources against
/// withheld paths with `Path::starts_with`, which is a component-wise prefix
/// test and not a filesystem one.
///
/// Deliberately **not** `fs::canonicalize`: on Windows that returns a
/// `\\?\`-prefixed verbatim path, which several tools read as a literal
/// directory name, and this crate targets Windows first-class. The cost is that
/// a symbolic link on the chain is not resolved here; the chain-validation that
/// refuses reparse points is `workspace_manager::validate_execution_root_chain`
/// and it runs before a worktree exists.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if matches!(
                    out.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The **exact** commit this worktree is at, as an object id.
///
/// "exact **detached** HEAD": a symbolic `ref: refs/heads/…` is resolved here,
/// on the coordinator, so the view carries an id rather than a name. A view
/// carrying the name would need the ref to exist inside it, and the refs are
/// exactly what the view withholds.
///
/// # Errors
///
/// [`UpstrokeError::Git`] when `HEAD` is missing, names a ref nothing resolves,
/// or does not resolve to an object id.
pub fn detached_head(layout: &GitLayout) -> Result<String, UpstrokeError> {
    let head_path = layout.git_dir.join("HEAD");
    let head = fs::read_to_string(&head_path)
        .map_err(|source| UpstrokeError::Io {
            path: head_path.clone(),
            source,
        })?
        .trim()
        .to_owned();

    let Some(name) = head.strip_prefix("ref:") else {
        return object_id(&head, &head_path);
    };
    let name = name.trim();

    // A loose ref, in the worktree's own half first (`refs/bisect/**` and
    // `HEAD` are per-worktree) and then in the shared half.
    for base in [&layout.git_dir, &layout.common_dir] {
        match fs::read_to_string(base.join(name)) {
            Ok(text) => return object_id(text.trim(), &base.join(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: base.join(name),
                    source,
                });
            }
        }
    }

    // Then `packed-refs`, whose lines are `<id> <name>`.
    let packed_path = layout.common_dir.join("packed-refs");
    if let Ok(packed) = fs::read_to_string(&packed_path) {
        for line in packed.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some((id, found)) = line.split_once(' ') {
                if found.trim() == name {
                    return object_id(id.trim(), &packed_path);
                }
            }
        }
    }

    Err(UpstrokeError::Git {
        message: format!(
            "`{}` names `{name}`, and nothing under `{}` or `{}` resolves it",
            head_path.display(),
            layout.git_dir.display(),
            layout.common_dir.display()
        ),
    })
}

/// A value that is an object id, or a refusal naming where it came from.
///
/// Forty characters for `sha1` and sixty-four for `sha256`, because
/// [`config_for`] carries the repository's `[extensions]` across and a
/// `sha256` repository is a thing this view has to be able to project.
fn object_id(value: &str, from: &Path) -> Result<String, UpstrokeError> {
    if matches!(value.len(), 40 | 64) && value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(value.to_owned());
    }
    Err(UpstrokeError::Git {
        message: format!(
            "`{}` holds `{value}`, which is not a Git object id; the container's Git view \
             carries an exact detached HEAD (DESIGN.md:612)",
            from.display()
        ),
    })
}

// ---------------------------------------------------------------------------
// The projection
// ---------------------------------------------------------------------------

/// The R19 disposable role-scoped Git view.
///
/// Implements [`GitView`], so the funnel — [`super::mount_git_view`] and
/// [`super::unmount_git_view`], the two `Container.MountGitView` /
/// `Container.UnmountGitView` APIs — is what a caller uses. Nothing here is
/// reachable except through those.
#[derive(Debug, Clone, Default)]
pub struct RoleGitView {
    trace: ContainerTrace,
    /// Where this view and the borrowed object store will be visible **to
    /// whoever reads the view**.
    ///
    /// `None` means "at the paths they are on this host", which is what a
    /// coordinator-side reader needs. [`super::exec::ContainerRunner`] sets it
    /// to the two **in-container** mount targets, because the reader is inside
    /// the container and a `gitdir:` line or an alternate naming a host path
    /// would name nothing there. That is the same class of defect as
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST`: a coordinator-host path serialized
    /// into something a boundary with its own filesystem has to read.
    ///
    /// One knob and not two, because the two files are read by the same reader
    /// and a view whose `gitdir:` was in-container and whose alternate was not
    /// would be half-projected — the shape nobody would notice until a gate ran.
    reader: Option<ReaderPaths>,
}

/// Where the view and the borrowed object store are, as the reader sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderPaths {
    /// The view directory.
    pub view: String,
    /// The borrowed object store.
    pub objects: String,
}

impl RoleGitView {
    /// A view whose actions are recorded in `trace`.
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self {
            trace,
            reader: None,
        }
    }

    /// Project for a reader that will see the view at `view` and the borrowed
    /// object store at `objects`.
    #[must_use]
    pub fn for_reader(mut self, view: impl Into<String>, objects: impl Into<String>) -> Self {
        self.reader = Some(ReaderPaths {
            view: view.into(),
            objects: objects.into(),
        });
        self
    }

    /// Where this view will tell a reader to look, given where it is on this
    /// host.
    #[must_use]
    pub fn reader_paths(&self, request: &GitViewRequest, layout: &GitLayout) -> ReaderPaths {
        self.reader.clone().unwrap_or_else(|| ReaderPaths {
            view: request.path.to_string_lossy().replace('\\', "/"),
            objects: layout.objects.to_string_lossy().replace('\\', "/"),
        })
    }
}

/// The files a projected view holds, in the order they are written.
///
/// Written out as a list so the census that proves the view carries nothing
/// else has something to compare against that is not the function that produced
/// it.
pub const PROJECTED_ENTRIES: &[&str] = &[
    "HEAD",
    "config",
    "index",
    "objects/info/alternates",
    "objects/pack",
    "refs/heads",
    "refs/tags",
    WORKTREE_GITFILE,
];

/// The names a view must never carry, each being a link back into the real
/// repository.
///
/// `commondir` and `gitdir` are how a linked worktree finds the repository;
/// `worktrees` is the registry of every other one; `packed-refs` is where the
/// engine's refs live once Git has packed them. DESIGN.md:612's sentence is
/// about exactly these.
pub const WITHHELD_ENTRIES: &[&str] = &[COMMONDIR, "gitdir", "worktrees", "packed-refs"];

impl GitView for RoleGitView {
    fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError> {
        create_dir(&request.path)?;

        if let Some(layout) = resolve(&request.workspace)? {
            let head = match &request.head {
                Some(head) => head.clone(),
                None => detached_head(&layout)?,
            };
            project(
                &request.path,
                &layout,
                &head,
                &self.reader_paths(request, &layout),
            )?;
        }

        self.trace.view(ViewAction::Materialized, &request.path);
        Ok(request.path.clone())
    }

    /// **Through the one racing removal** (`PR6-CORRECTNESS-009`).
    ///
    /// `crash_reconstruction` requires "every step idempotent and tolerant of
    /// already-gone so **two concurrent reclaimers converge**", and on Windows
    /// the loser of that race does not get `NotFound`: a directory whose last
    /// handle has closed is *delete-pending*, and `remove_dir_all` reports
    /// `PermissionDenied` until the name actually goes away. This implementation
    /// tolerated only `NotFound`, so one of two converging write commands
    /// refused. [`super::DisposableDirView`] had already been repaired; this
    /// one is the projection a real run mounts, and the concurrent-census
    /// fixtures all used the other.
    ///
    /// **Tolerating `PermissionDenied` outright would be the wrong repair** and
    /// it is worth saying why, because it is the smaller diff: a view that is
    /// genuinely protected — an open handle inside it on Windows, a read-only
    /// parent on Unix — would then be reported discarded, the census would go on
    /// to remove the intent, and admission would proceed over R19 residue that
    /// nothing will ever reclaim, because the record naming it is gone.
    /// [`super::racing_removal`] retries and then **fails**, which is the
    /// fail-closed shape: a delete-pending name disappears within a few
    /// attempts, and a protected one still refuses after all of them.
    fn discard(&self, path: &Path) -> Result<(), UpstrokeError> {
        super::racing_removal(path, || fs::remove_dir_all(path))?;
        self.trace.view(ViewAction::Discarded, path);
        Ok(())
    }
}

/// Write the projection into `view`.
fn project(
    view: &Path,
    layout: &GitLayout,
    head: &str,
    reader: &ReaderPaths,
) -> Result<(), UpstrokeError> {
    // Exact detached HEAD: an id, never a name.
    write_file(&view.join("HEAD"), format!("{head}\n").as_bytes())?;

    // The repository format, and any extension the object store depends on.
    write_file(&view.join("config"), config_for(layout)?.as_bytes())?;

    // Exact index. A worktree with no index yet is a real state — nothing has
    // been staged — and an absent index is what Git expects then, not an empty
    // file, which Git reads as a corrupt one.
    match fs::read(layout.git_dir.join("index")) {
        Ok(bytes) => write_file(&view.join("index"), &bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: layout.git_dir.join("index"),
                source,
            });
        }
    }

    // And the half a **split** index keeps beside it. See
    // [`SHARED_INDEX_PREFIX`]: without this the projected index is one Git
    // refuses to open, so "exact index" holds only for repositories that do not
    // use `core.splitIndex`.
    copy_shared_indexes(&layout.git_dir, view)?;

    // Read-only objects: borrowed through Git's own alternate mechanism, which
    // Git resolves through and never writes to. Every object this view's reader
    // creates lands in `objects/` below, which the release prunes.
    create_dir(&view.join("objects").join("info"))?;
    create_dir(&view.join("objects").join("pack"))?;
    write_file(
        &view.join(ALTERNATES),
        format!("{}\n", reader.objects).as_bytes(),
    )?;

    // No engine refs: the two directories Git requires, both empty, and no
    // `packed-refs`.
    create_dir(&view.join("refs").join("heads"))?;
    create_dir(&view.join("refs").join("tags"))?;

    // The overlay itself: what `<workspace>/.git` becomes.
    write_file(
        &view.join(WORKTREE_GITFILE),
        format!("{GITDIR_PREFIX} {}\n", reader.view).as_bytes(),
    )?;
    Ok(())
}

/// Copy every `sharedindex.*` beside the source index into the view, and answer
/// which names were copied.
///
/// **Every one, not the one the `link` extension names.** Reading that name out
/// of the index means parsing the index: the extension block sits after the
/// entries, index v4 prefix-compresses entry paths, and a scan for the four
/// bytes `link` matches a path that happens to contain them. A wrong parse here
/// silently drops the file again, which is the failure being repaired. Git
/// keeps at most a small number of these — the live one and, briefly, the one
/// it replaced — they are index data of the repository the view already
/// borrows, and the whole directory is discarded with the invocation.
///
/// # Errors
///
/// [`UpstrokeError::Io`] when the Git directory or a shared index cannot be read.
fn copy_shared_indexes(git_dir: &Path, view: &Path) -> Result<Vec<String>, UpstrokeError> {
    let entries = match fs::read_dir(git_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: git_dir.to_path_buf(),
                source,
            });
        }
    };
    let mut copied = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| UpstrokeError::Io {
            path: git_dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(SHARED_INDEX_PREFIX) {
            continue;
        }
        let from = entry.path();
        let bytes = match fs::read(&from) {
            Ok(bytes) => bytes,
            // Git replaced it between the listing and the read; the `index`
            // this view carries names the one that is there, and a shared index
            // that went away under the scan was not it.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(UpstrokeError::Io { path: from, source }),
        };
        write_file(&view.join(&name), &bytes)?;
        copied.push(name);
    }
    copied.sort();
    Ok(copied)
}

/// The view's `config`.
///
/// Minimal by construction rather than copied: the repository's own config
/// carries remotes, URLs and credential helpers, and a view that copied it
/// would hand a container the operator's forge credentials — the opposite of
/// what R19 is for. What *is* carried over is the repository format and the
/// `[extensions]` section, because those describe the object store the view
/// borrows: a `sha256` repository read as `sha1` is a repository read wrong,
/// and an unknown extension declared is a Git that refuses loudly instead of
/// one that misreads.
fn config_for(layout: &GitLayout) -> Result<String, UpstrokeError> {
    let mut config = String::from("[core]\n\tbare = false\n\tlogallrefupdates = false\n");
    let source = match fs::read_to_string(layout.common_dir.join("config")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: layout.common_dir.join("config"),
                source,
            });
        }
    };

    let mut version = "0".to_owned();
    let mut extensions = Vec::new();
    let mut in_extensions = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_extensions = trimmed.eq_ignore_ascii_case("[extensions]");
            continue;
        }
        if in_extensions && !trimmed.is_empty() && !trimmed.starts_with('#') {
            extensions.push(trimmed.to_owned());
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            if key.trim().eq_ignore_ascii_case("repositoryformatversion") {
                version = value.trim().to_owned();
            }
        }
    }
    config.push_str(&format!("\trepositoryformatversion = {version}\n"));
    if !extensions.is_empty() {
        config.push_str("[extensions]\n");
        for entry in extensions {
            config.push_str(&format!("\t{entry}\n"));
        }
    }
    Ok(config)
}

fn create_dir(path: &Path) -> Result<(), UpstrokeError> {
    fs::create_dir_all(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), UpstrokeError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    let mut file = fs::File::create(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(bytes).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })
}

// -- test-only declarations ----------------------------------------------
// At the BOTTOM: `effects::production_region`, which
// `effects::externally_reachable_fns` and the three censuses in `super::exec`
// still use, cuts a source at its first `#[cfg(test)]`
// (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

/// Real temporary Git repositories, built through the Runner.
///
/// `decisions.tests_acceptance.determinism` says "**real temporary Git
/// repositories**", and this module is the one that builds them for the
/// container lane. Every `git` here goes through
/// [`crate::runner::host::HostRunner`] rather than through a
/// `std::process::Command` of its own — which is the same rule the production
/// tree obeys ("every CLI and gate process executes through Runner") and, in
/// passing, keeps `std::process::Command` out of this module's lint set.
///
/// `pub(crate)` and declared here rather than inside `mod tests`, because
/// `super::exec`'s suite builds the same repositories and two copies of a
/// fixture are two fixtures that drift.
///
/// **A `pub(crate) mod` used to read as production to the source censuses in
/// `src/runner/mod.rs`.** Their region was `runner::tests::production_region`,
/// whose predicate was that the line after the cfg attribute starts with a bare
/// `mod` keyword; a visibility qualifier in front of it defeated that, and this
/// block was scanned as production. **That reader is deleted.** All four
/// whole-tree censuses now share `effects::production_code`, which finds the
/// item's extent by delimiter matching rather than by reading a line, so
/// `#[cfg(test)] pub(crate) mod fixtures { … }` is removed like any other
/// configured item — measured on this file, which contributes nothing to any of
/// them.
///
/// Nothing here constructs a process, a spawn, a timed run, a role literal or a
/// request literal anyway, and every `git` goes through the gate-request builder
/// and the host runner. That is now a discipline rather than a requirement, and
/// it is kept: it is what makes the block safe to read as production if a future
/// region ever does.
///
/// **All four censuses blank comments *and* string literals now**
/// (`PR5-R1-PROCESS-START-CENSUS-UNSTRIPPED`, closed), so a doc comment here
/// that names one of their needles no longer changes an expected count. It once
/// did: the paragraph above, in its first spelling, added a phantom row for this
/// file to two of them — the sixth occurrence of `PR4-CENSUS-COMMENT-ORACLE` on
/// this project, and the one that finally moved the blanking into the shared
/// region instead of into each census.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::agent::ProcessOutput;
    use crate::runner::host::HostRunner;
    use crate::runner::invocation::AttemptRole;
    use crate::runner::{CommandSpec, InvocationId, Runner, gate_request};
    use crate::topology::events::{AttemptNumber, GenerationId};
    use crate::topology::registry::TaskKey;

    /// A scratch directory, in the idiom of `runner::container::tests::scratch`.
    pub(crate) fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "upstroke-view-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// One `git` invocation, in `cwd`, through the host runner.
    pub(crate) fn git(cwd: &Path, args: &[&str]) -> ProcessOutput {
        let mut spec = CommandSpec::new("git");
        // A fixed identity, so a commit is a function of its inputs rather than
        // of whoever's `~/.gitconfig` the suite runs under — and so a machine
        // with no identity configured can still build a fixture.
        for fixed in [
            "-c",
            "user.name=upstroke-test",
            "-c",
            "user.email=upstroke@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ] {
            spec = spec.arg(fixed);
        }
        for arg in args {
            spec = spec.arg(*arg);
        }
        HostRunner::new()
            .run(&gate_request(
                spec,
                cwd.to_path_buf(),
                Duration::from_secs(60),
                InvocationId::attempt(
                    TaskKey(0),
                    GenerationId(0),
                    AttemptNumber(1),
                    AttemptRole::Gate(0),
                    0,
                ),
            ))
            .expect("git runs through the host runner")
    }

    /// One `git` invocation that must succeed, with its trimmed stdout.
    pub(crate) fn git_ok(cwd: &Path, args: &[&str]) -> String {
        let output = git(cwd, args);
        assert_eq!(
            output.code,
            Some(0),
            "`git {args:?}` in {} exited {:?}: {}",
            cwd.display(),
            output.code,
            output.stderr
        );
        output.stdout.trim().to_owned()
    }

    /// A repository with two commits, at `dir`.
    ///
    /// Returns `(head, previous)` — two distinct object ids, so a test can move
    /// a worktree between them and see the view follow.
    pub(crate) fn repository(dir: &Path) -> (String, String) {
        std::fs::create_dir_all(dir).expect("the repository directory");
        git_ok(dir, &["init", "-q"]);
        std::fs::write(dir.join("first.txt"), "one\n").expect("a file");
        git_ok(dir, &["add", "first.txt"]);
        git_ok(dir, &["commit", "-q", "-m", "first"]);
        let previous = git_ok(dir, &["rev-parse", "HEAD"]);
        std::fs::write(dir.join("second.txt"), "two\n").expect("a second file");
        git_ok(dir, &["add", "second.txt"]);
        git_ok(dir, &["commit", "-q", "-m", "second"]);
        let head = git_ok(dir, &["rev-parse", "HEAD"]);
        assert_ne!(head, previous, "the two commits are one commit");
        (head, previous)
    }

    /// A detached linked worktree of `repo` at `at`, checked out at `commit`.
    pub(crate) fn worktree(repo: &Path, at: &Path, commit: &str) {
        git_ok(
            repo,
            &[
                "worktree",
                "add",
                "--detach",
                "-q",
                &at.to_string_lossy(),
                commit,
            ],
        );
    }

    /// Engine refs, of the shape `src/workspace_manager.rs` writes.
    pub(crate) fn engine_refs(repo: &Path, commit: &str) -> Vec<String> {
        let names = vec![
            "refs/upstroke/runs/01RUN/candidates/k0/1".to_owned(),
            "refs/upstroke/runs/01RUN/integration".to_owned(),
            "refs/upstroke/prepared/01RUN/0-1".to_owned(),
        ];
        for name in &names {
            git_ok(repo, &["update-ref", name, commit]);
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{engine_refs, git, git_ok, repository, scratch, worktree};
    use super::*;
    use crate::runner::container::runtime::TraceEntry;

    /// A linked worktree has three Git directories at three places, and a main
    /// worktree has two of the three at one.
    ///
    /// Second field held constant: the repository — the *same* repository is
    /// resolved twice — so what varies is only which worktree is asked.
    #[test]
    fn a_linked_worktrees_three_git_directories_resolve_to_three_distinct_places() {
        let root = scratch("layout");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let linked = root.join("tasks").join("k0-g0");
        worktree(&repo, &linked, &head);

        let main = resolve(&repo).expect("resolves").expect("a repository");
        assert_eq!(main.git_dir, main.common_dir, "a main worktree is its own");
        assert_eq!(main.objects, main.common_dir.join("objects"));

        let linked_layout = resolve(&linked).expect("resolves").expect("a worktree");
        assert_ne!(
            linked_layout.git_dir, linked_layout.common_dir,
            "a linked worktree's `.git` points back into the real repository — \
             which is the sentence this whole module exists for"
        );
        assert_eq!(
            linked_layout.common_dir.canonicalize().expect("canonical"),
            main.common_dir.canonicalize().expect("canonical"),
            "the shared half is the repository's own"
        );
        assert!(
            linked_layout.git_dir.starts_with(&linked_layout.common_dir),
            "{:?}",
            linked_layout.git_dir
        );
        // Three distinct places, counted rather than described.
        let places: std::collections::BTreeSet<PathBuf> = [
            linked_layout.git_dir.clone(),
            linked_layout.common_dir.clone(),
            linked_layout.objects.clone(),
        ]
        .into_iter()
        .collect();
        assert_eq!(places.len(), 3);
        assert!(linked_layout.objects.is_dir(), "the object store is real");
    }

    /// A workspace with no repository is a real state, and it still gets a
    /// view.
    ///
    /// R19's granularity is "per container invocation (**incl. shell and agent
    /// probes**)", and a probe's workspace is a scratch directory. A
    /// `materialize` that refused there would make every probe unable to start.
    #[test]
    fn a_workspace_with_no_repository_has_no_layout_and_still_gets_a_view() {
        let root = scratch("no-repo");
        let workspace = root.join("scratch");
        std::fs::create_dir_all(&workspace).expect("a workspace");
        assert_eq!(resolve(&workspace).expect("resolves"), None);

        let trace = ContainerTrace::recording();
        let view = RoleGitView::new(trace.clone());
        let path = view
            .materialize(&GitViewRequest {
                path: root.join("view"),
                workspace,
                head: None,
            })
            .expect("a probe still gets its view directory");
        assert!(path.is_dir());
        assert!(!path.join("HEAD").exists(), "and nothing is projected");
        assert_eq!(
            std::fs::read_dir(&path).expect("read the view").count(),
            0,
            "an empty view is empty"
        );
        assert!(
            trace
                .entries()
                .iter()
                .any(|entry| matches!(entry, TraceEntry::View { .. })),
            "the view action is recorded whether or not anything was projected"
        );
    }

    /// The view carries the worktree's **exact** detached HEAD and index.
    ///
    /// The expected head comes from `git rev-parse` — Git's own answer, run by
    /// the fixture — and never from [`detached_head`], which is the function
    /// this pins. The second commit is the reason the fixture builds two: a
    /// view whose HEAD was a constant, or was the repository's `HEAD` rather
    /// than the worktree's, passes with one.
    ///
    /// Second field held constant: the workspace path and the repository; what
    /// varies is which commit the worktree is at.
    #[test]
    fn the_view_carries_the_exact_detached_head_and_index_of_the_worktree() {
        let root = scratch("exact");
        let repo = root.join("repo");
        let (head, previous) = repository(&repo);

        for (tag, commit) in [("at-head", &head), ("at-previous", &previous)] {
            let workspace = root.join("tasks").join(tag);
            worktree(&repo, &workspace, commit);
            let layout = resolve(&workspace).expect("resolves").expect("a worktree");

            // The oracle is git's, not ours.
            let by_git = git_ok(&workspace, &["rev-parse", "HEAD"]);
            assert_eq!(&by_git, commit, "the fixture put the worktree elsewhere");
            assert_eq!(
                detached_head(&layout).expect("HEAD resolves"),
                by_git,
                "{tag}: the view's HEAD is not the worktree's"
            );

            let view_path = root.join("views").join(tag);
            RoleGitView::new(ContainerTrace::off())
                .materialize(&GitViewRequest {
                    path: view_path.clone(),
                    workspace: workspace.clone(),
                    head: None,
                })
                .expect("materializes");

            assert_eq!(
                std::fs::read_to_string(view_path.join("HEAD")).expect("HEAD"),
                format!("{by_git}\n"),
                "{tag}: an id, on its own line — not `ref: …`"
            );
            // Exact index: the bytes, not a rebuild.
            let source_index = std::fs::read(layout.git_dir.join("index")).expect("the index");
            assert!(!source_index.is_empty(), "the fixture staged nothing");
            assert_eq!(
                std::fs::read(view_path.join("index")).expect("the view's index"),
                source_index,
                "{tag}: the index is copied byte for byte"
            );
        }

        // The two views really do differ, so the assertions above are about the
        // worktree rather than about a constant.
        assert_ne!(
            std::fs::read_to_string(root.join("views").join("at-head").join("HEAD")).expect("HEAD"),
            std::fs::read_to_string(root.join("views").join("at-previous").join("HEAD"))
                .expect("HEAD"),
        );
    }

    /// A **symbolic** HEAD is resolved to an object id before it reaches the
    /// view.
    ///
    /// "exact **detached** HEAD". A view carrying `ref: refs/heads/main` would
    /// need that ref to exist inside it, and the refs are exactly what the view
    /// withholds — so a tool reading such a view sees an unborn branch.
    #[test]
    fn a_symbolic_head_is_resolved_to_an_object_id_before_it_reaches_the_view() {
        let root = scratch("symbolic");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        // The main worktree's HEAD *is* symbolic.
        let raw = std::fs::read_to_string(repo.join(".git").join("HEAD")).expect("HEAD");
        assert!(
            raw.starts_with("ref:"),
            "the fixture is not symbolic: {raw}"
        );

        let layout = resolve(&repo).expect("resolves").expect("a repository");
        assert_eq!(detached_head(&layout).expect("resolves"), head);

        // And through packed-refs, which is where the loose ref goes when Git
        // packs it — the other half of the resolution and a separate branch of
        // the code.
        git_ok(&repo, &["pack-refs", "--all"]);
        assert!(
            !repo
                .join(".git")
                .join("refs")
                .join("heads")
                .join("main")
                .exists()
                || std::fs::read_dir(repo.join(".git").join("refs").join("heads"))
                    .expect("refs/heads")
                    .count()
                    == 0,
            "the fixture did not pack the ref, so the packed-refs branch is untested"
        );
        assert_eq!(
            detached_head(&layout).expect("resolves through packed-refs"),
            head
        );

        let view_path = root.join("view");
        RoleGitView::new(ContainerTrace::off())
            .materialize(&GitViewRequest {
                path: view_path.clone(),
                workspace: repo.clone(),
                head: None,
            })
            .expect("materializes");
        assert_eq!(
            std::fs::read_to_string(view_path.join("HEAD")).expect("HEAD"),
            format!("{head}\n")
        );
    }

    /// A HEAD that is not an object id refuses rather than producing a view of
    /// nothing.
    #[test]
    fn a_head_that_names_nothing_refuses() {
        let root = scratch("unborn");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("the directory");
        git_ok(&repo, &["init", "-q"]);
        let layout = resolve(&repo).expect("resolves").expect("a repository");
        let refusal = detached_head(&layout).expect_err("an unborn branch resolves to nothing");
        assert!(
            refusal.to_string().contains("refs/heads/"),
            "the refusal does not say what it could not resolve: {refusal}"
        );

        // And a HEAD holding something that is not an id at all.
        std::fs::write(layout.git_dir.join("HEAD"), "not-an-object-id\n").expect("plant");
        let refusal = detached_head(&layout).expect_err("refuses");
        assert!(
            refusal.to_string().contains("not a Git object id"),
            "{refusal}"
        );
    }

    /// No engine refs, and no link back into the real repository.
    ///
    /// The repository is loaded with the three shapes of engine ref
    /// `src/workspace_manager.rs` writes — a candidate, an integration ref and
    /// a prepared pin — so the count on the repository side is the control: a
    /// view that carried them would differ from zero, and a fixture that
    /// planted none would not.
    ///
    /// Second field held constant: the worktree and its HEAD; what varies is
    /// which repository the refs are read from.
    #[test]
    fn the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository() {
        let root = scratch("no-refs");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let planted = engine_refs(&repo, &head);
        assert_eq!(planted.len(), 3);
        let workspace = root.join("tasks").join("k0-g0");
        worktree(&repo, &workspace, &head);
        // Pack them, so the view cannot avoid them merely by not copying loose
        // files.
        git_ok(&repo, &["pack-refs", "--all"]);
        let in_repo = git_ok(&repo, &["for-each-ref", "--format=%(refname)"]);
        for name in &planted {
            assert!(in_repo.contains(name.as_str()), "the control: {in_repo}");
        }

        let view_path = root.join("view");
        RoleGitView::new(ContainerTrace::off())
            .materialize(&GitViewRequest {
                path: view_path.clone(),
                workspace,
                head: None,
            })
            .expect("materializes");

        // Every name that would link back, by name rather than by inspection.
        assert_eq!(WITHHELD_ENTRIES.len(), 4);
        for withheld in WITHHELD_ENTRIES {
            assert!(
                !view_path.join(withheld).exists(),
                "the view carries `{withheld}`, which links back into the repository"
            );
        }
        // The refs the view does carry: two empty directories and nothing else.
        for dir in ["refs/heads", "refs/tags"] {
            let entries = std::fs::read_dir(view_path.join(dir)).expect(dir).count();
            assert_eq!(entries, 0, "`{dir}` is not empty");
        }
        // And the config names no remote, no URL and no credential helper: a
        // view that copied the repository's config would hand a container the
        // operator's forge credentials.
        let config = std::fs::read_to_string(view_path.join("config")).expect("config");
        for forbidden in ["[remote", "url", "credential", "[branch"] {
            assert!(
                !config.contains(forbidden),
                "the view's config names `{forbidden}`: {config}"
            );
        }
        // The projection is exactly the entries the module declares.
        for entry in PROJECTED_ENTRIES {
            assert!(
                view_path.join(entry).exists(),
                "the projection is missing `{entry}`"
            );
        }
    }

    /// `proof_tests[1]`: a Git-dependent tool sees only the role view.
    ///
    /// Real Git, over the real projection, on the host — so this holds on every
    /// platform the suite runs on, with no container runtime. The container
    /// half is `exec::tests::real_docker_a_git_dependent_gate_sees_only_the_role_view`,
    /// which runs the same commands inside a container over the same view.
    ///
    /// Second field held constant: the commands are the *same* commands run
    /// against the worktree's own Git directory first — that is the control
    /// pair, so "the view answers" and "the view withholds" are both measured
    /// against a run that is known to do the opposite.
    #[test]
    fn a_git_dependent_tool_reads_the_role_view_and_cannot_see_the_engines_refs() {
        let root = scratch("git-tool");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let planted = engine_refs(&repo, &head);
        let workspace = root.join("tasks").join("k0-g0");
        worktree(&repo, &workspace, &head);

        let view_path = root.join("view");
        RoleGitView::new(ContainerTrace::off())
            .materialize(&GitViewRequest {
                path: view_path.clone(),
                workspace: workspace.clone(),
                head: None,
            })
            .expect("materializes");

        let view = view_path.to_string_lossy().into_owned();
        let work = workspace.to_string_lossy().into_owned();
        let through_view = |args: &[&str]| -> crate::agent::ProcessOutput {
            let mut all = vec!["--git-dir", view.as_str(), "--work-tree", work.as_str()];
            all.extend_from_slice(args);
            git(&workspace, &all)
        };

        // Objects resolve — through the alternate, which is Git's own
        // read-only borrow.
        assert_eq!(
            through_view(&["rev-parse", "HEAD"]).stdout.trim(),
            head,
            "the view cannot see its own HEAD"
        );
        assert_eq!(
            through_view(&["log", "-1", "--format=%s"]).stdout.trim(),
            "second",
            "the objects are not reachable, so this is a view of nothing"
        );
        assert_eq!(
            through_view(&["cat-file", "-t", &head]).stdout.trim(),
            "commit"
        );
        // The index is exact, so a clean worktree reads clean.
        assert_eq!(
            through_view(&["status", "--porcelain"]).stdout.trim(),
            "",
            "the index the view carries is not the worktree's"
        );
        // The Git directory the tool reports is the view, not the coordinator's.
        assert_eq!(
            std::path::Path::new(
                through_view(&["rev-parse", "--absolute-git-dir"])
                    .stdout
                    .trim()
            )
            .canonicalize()
            .expect("canonical"),
            view_path.canonicalize().expect("canonical")
        );

        // And no engine ref is visible.
        assert_eq!(
            through_view(&["for-each-ref", "--format=%(refname)"])
                .stdout
                .trim(),
            "",
            "the view carries refs"
        );
        for name in &planted {
            let found = through_view(&["rev-parse", "--verify", "--quiet", name.as_str()]);
            assert_ne!(
                found.code,
                Some(0),
                "`{name}` resolves inside the role view: {}",
                found.stdout
            );
            // The control: the *same* command against the worktree's own Git
            // directory does resolve it, so the assertion above is about the
            // view rather than about the command.
            assert_eq!(
                git_ok(&workspace, &["rev-parse", "--verify", name.as_str()]),
                head
            );
        }

        // The coordinator's refs are unchanged by any of it.
        let after = git_ok(&repo, &["for-each-ref", "--format=%(refname)"]);
        for name in &planted {
            assert!(after.contains(name.as_str()));
        }
    }

    /// A write through the view lands in the view, never in the repository.
    ///
    /// "**read-only objects** … without exposing or **mutating** the
    /// coordinator's refs". The alternate is what Git reads through and never
    /// writes to, so an object created against the view goes into the view's
    /// own `objects/` — which the release prunes.
    #[test]
    fn an_object_written_through_the_view_lands_in_the_view_and_not_in_the_repository() {
        let root = scratch("disposable");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let workspace = root.join("tasks").join("k0-g0");
        worktree(&repo, &workspace, &head);

        let view_path = root.join("view");
        let view = RoleGitView::new(ContainerTrace::off());
        view.materialize(&GitViewRequest {
            path: view_path.clone(),
            workspace: workspace.clone(),
            head: None,
        })
        .expect("materializes");

        let before = count_objects(&repo.join(".git").join("objects"));
        let view_dir = view_path.to_string_lossy().into_owned();
        let work = workspace.to_string_lossy().into_owned();
        let written = git_ok(
            &workspace,
            &[
                "--git-dir",
                view_dir.as_str(),
                "--work-tree",
                work.as_str(),
                "hash-object",
                "-w",
                "--stdin-paths",
            ],
        );
        // `--stdin-paths` with no stdin writes nothing; use a real file instead.
        assert!(written.is_empty());
        std::fs::write(workspace.join("third.txt"), "three\n").expect("a file");
        let id = git_ok(
            &workspace,
            &[
                "--git-dir",
                view_dir.as_str(),
                "--work-tree",
                work.as_str(),
                "hash-object",
                "-w",
                "third.txt",
            ],
        );
        assert_eq!(id.len(), 40, "{id}");

        assert_eq!(
            count_objects(&repo.join(".git").join("objects")),
            before,
            "an object written through the view reached the coordinator's store"
        );
        assert!(
            count_objects(&view_path.join("objects")) > 0,
            "and it did not reach the view's own store either"
        );

        // Discarded, twice: idempotent, and nothing is left.
        for round in 0..2 {
            view.discard(&view_path)
                .unwrap_or_else(|error| panic!("round {round}: {error}"));
            assert!(!view_path.exists());
        }
        // The repository is untouched by the discard.
        assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head);
    }

    /// Loose objects under `objects/`, ignoring `info` and `pack` metadata.
    fn count_objects(objects: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(objects) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name != "info" && name != "pack" && entry.path().is_dir()
            })
            .map(|entry| {
                std::fs::read_dir(entry.path())
                    .map(|inner| inner.count())
                    .unwrap_or(0)
            })
            .sum()
    }

    /// The `gitdir:` line and the alternate name the paths **the reader** will
    /// see, not the ones the coordinator sees.
    ///
    /// A coordinator-host path written into a file a container reads is
    /// `PR4-ADAPTER-RESOLVES-ON-THE-HOST`'s shape, one layer down: it names
    /// nothing inside the image. Both files are checked, because a view whose
    /// `gitdir:` was in-container and whose alternate was not would be
    /// half-projected — a Git that finds the view and then cannot find an
    /// object, which reads as a corrupt repository rather than as a mistake
    /// here.
    ///
    /// Second field held constant: the workspace and its layout; what varies is
    /// who the reader is.
    #[test]
    fn the_projection_names_the_paths_the_reader_will_see_and_not_the_hosts() {
        let root = scratch("reader");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let workspace = root.join("tasks").join("k0-g0");
        worktree(&repo, &workspace, &head);
        let layout = resolve(&workspace).expect("resolves").expect("a worktree");
        assert!(
            layout.dot_git_is_file,
            "a linked worktree's `.git` is a file, and the mount shape follows it"
        );

        // The coordinator's reader: the host paths.
        let host_view = root.join("view-host");
        RoleGitView::new(ContainerTrace::off())
            .materialize(&GitViewRequest {
                path: host_view.clone(),
                workspace: workspace.clone(),
                head: None,
            })
            .expect("materializes");
        let host_alternate =
            std::fs::read_to_string(host_view.join(ALTERNATES)).expect("alternates");
        let host_gitfile =
            std::fs::read_to_string(host_view.join(WORKTREE_GITFILE)).expect("gitfile");
        assert_eq!(
            host_alternate.trim(),
            layout.objects.to_string_lossy().replace('\\', "/"),
            "the default alternate is the store's path on this host"
        );
        assert_eq!(
            host_gitfile.trim(),
            format!("gitdir: {}", host_view.to_string_lossy().replace('\\', "/"))
        );

        // A container's reader: the in-container mount targets.
        let container_view = root.join("view-container");
        RoleGitView::new(ContainerTrace::off())
            .for_reader("/upstroke/gitview", "/upstroke/gitobjects")
            .materialize(&GitViewRequest {
                path: container_view.clone(),
                workspace,
                head: None,
            })
            .expect("materializes");
        assert_eq!(
            std::fs::read_to_string(container_view.join(ALTERNATES))
                .expect("alternates")
                .trim(),
            "/upstroke/gitobjects"
        );
        assert_eq!(
            std::fs::read_to_string(container_view.join(WORKTREE_GITFILE))
                .expect("gitfile")
                .trim(),
            "gitdir: /upstroke/gitview"
        );
        assert_ne!(
            host_alternate,
            std::fs::read_to_string(container_view.join(ALTERNATES)).expect("alternates"),
            "the two readers were given the same path, so one of them is wrong"
        );
        assert_ne!(
            host_gitfile,
            std::fs::read_to_string(container_view.join(WORKTREE_GITFILE)).expect("gitfile"),
        );
    }

    /// The `.git` kind is read from the worktree, and it decides the mount
    /// shape.
    ///
    /// Measured against `docker` 29.7.2: a directory cannot be bind-mounted
    /// onto a file. A `GitLayout` that always reported one kind would produce a
    /// container that fails at `runc create` for the other — a failure with no
    /// test above it, because every fixture in this file would have used the
    /// kind that happened to work.
    ///
    /// Second field held constant: the repository; what varies is which of its
    /// worktrees is asked.
    #[test]
    fn the_dot_git_kind_is_read_from_the_worktree_and_takes_both_values() {
        let root = scratch("dotgit-kind");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let linked = root.join("tasks").join("k0-g0");
        worktree(&repo, &linked, &head);

        let main = resolve(&repo).expect("resolves").expect("a repository");
        assert!(
            !main.dot_git_is_file,
            "a main worktree's `.git` is a directory"
        );
        let linked_layout = resolve(&linked).expect("resolves").expect("a worktree");
        assert!(linked_layout.dot_git_is_file);
        // Both values are taken, which is what makes the mount-shape match in
        // `exec::ContainerRunner::mounts` reachable on both arms.
        let kinds: std::collections::BTreeSet<bool> =
            [main.dot_git_is_file, linked_layout.dot_git_is_file]
                .into_iter()
                .collect();
        assert_eq!(kinds.len(), 2);
    }
}

// ---------------------------------------------------------------------------
// R3b: the half a split index keeps beside itself
// ---------------------------------------------------------------------------

#[cfg(test)]
mod split_index_tests {
    use super::fixtures::{git_ok, repository, scratch, worktree};
    use super::*;

    /// A **split** index projects whole, and real Git reads it.
    ///
    /// `PR6-CORRECTNESS-010`. `core.splitIndex` — which
    /// `git update-index --split-index` sets and which `feature.manyFiles`
    /// turns on — moves most of the index's entries into
    /// `<git-dir>/sharedindex.<oid>` and leaves `index` holding a `link`
    /// extension naming it. The projection copied `index` alone, so the view's
    /// index named a file that was not there and Git refused to open it at all:
    ///
    /// ```text
    /// fatal: <view>/sharedindex.<oid>: index file open failed: No such file or directory
    /// ```
    ///
    /// DESIGN.md:612 is "**exact** detached HEAD/index … so Git-dependent tools
    /// work", and a gate reading that view failed on the repository's Git
    /// configuration rather than on its own subject. No fixture created a split
    /// index, so the whole class was invisible.
    ///
    /// The grid is **{ordinary index, split index} × {is the view readable}**,
    /// and the ordinary cell is the control: it carries no `sharedindex.*`, so
    /// "the view works" in the split cell is attributable to the copy and not
    /// to Git ignoring the extension.
    ///
    /// The oracle is **Git**, not this module: each cell runs
    /// `git ls-files` against the projected view and compares it with the same
    /// command against the source worktree. A projection that produced an index
    /// this code was happy with and Git was not would fail here.
    ///
    /// Second field held constant: the same repository, the same worktree, the
    /// same staged file and the same HEAD in both cells; only whether the index
    /// was split moves.
    #[test]
    fn a_split_index_projects_with_the_shared_half_it_links_to() {
        for (label, split) in [("ordinary", false), ("split", true)] {
            let root = scratch(&format!("split-index-{label}"));
            let repo = root.join("repo");
            let (head, _) = repository(&repo);
            let workspace = root.join("tasks").join("k0-g0");
            worktree(&repo, &workspace, &head);

            // Something staged, so the index is not the empty one and
            // `ls-files` has an answer that can differ.
            std::fs::write(workspace.join("staged.txt"), "staged\n").expect("a file");
            git_ok(&workspace, &["add", "staged.txt"]);
            if split {
                git_ok(&workspace, &["update-index", "--split-index"]);
            }

            let layout = resolve(&workspace)
                .expect("resolves")
                .expect("a repository");
            let shared: Vec<String> = std::fs::read_dir(&layout.git_dir)
                .expect("the worktree git dir")
                .filter_map(|entry| {
                    let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                    name.starts_with(SHARED_INDEX_PREFIX).then_some(name)
                })
                .collect();
            // The premise of each cell, measured rather than assumed: only the
            // split one has a shared half.
            assert_eq!(
                !shared.is_empty(),
                split,
                "[{label}] the fixture did not build the index it names: {shared:?}"
            );

            let view_path = root.join("view");
            RoleGitView::new(ContainerTrace::off())
                .materialize(&GitViewRequest {
                    path: view_path.clone(),
                    workspace: workspace.clone(),
                    head: None,
                })
                .expect("materializes");

            // Every shared half the source had, the view has — by name.
            for name in &shared {
                assert!(
                    view_path.join(name).exists(),
                    "[{label}] the projection dropped `{name}`, which its own `index` links to"
                );
            }
            let in_view: Vec<String> = std::fs::read_dir(&view_path)
                .expect("the view")
                .filter_map(|entry| {
                    let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                    name.starts_with(SHARED_INDEX_PREFIX).then_some(name)
                })
                .collect();
            assert_eq!(
                in_view.len(),
                shared.len(),
                "[{label}] the view carries a different number of shared indexes than its source"
            );

            // The oracle: Git itself, over the projected view, compared with
            // Git over the source worktree.
            let through_view = crate::runner::container::view::fixtures::git(
                &workspace,
                &[
                    "--git-dir",
                    &view_path.to_string_lossy(),
                    "--work-tree",
                    &workspace.to_string_lossy(),
                    "ls-files",
                    "--cached",
                ],
            );
            assert_eq!(
                through_view.code,
                Some(0),
                "[{label}] Git could not read the projected index: {}",
                through_view.stderr
            );
            let expected = git_ok(&workspace, &["ls-files", "--cached"]);
            assert_eq!(
                through_view.stdout.trim(),
                expected,
                "[{label}] the projected index is not the source worktree's exact index"
            );
            assert!(
                expected.contains("staged.txt"),
                "[{label}] the fixture staged nothing, so the comparison above is vacuous"
            );

            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
