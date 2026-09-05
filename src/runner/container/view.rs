//! Extended notes: `docs/internals/runner/container/view.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types, clippy::disallowed_macros)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;

use super::runtime::{ContainerTrace, ViewAction};
use super::{GitView, GitViewRequest};

const GITDIR_PREFIX: &str = "gitdir:";

const COMMONDIR: &str = "commondir";

const DOT_GIT: &str = ".git";

const OBJECTS: &str = "objects";

const ALTERNATES: &str = "objects/info/alternates";

pub const WORKTREE_GITFILE: &str = "worktree.gitfile";

pub const SHARED_INDEX_PREFIX: &str = "sharedindex.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLayout {
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub objects: PathBuf,
    pub dot_git_is_file: bool,
}

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

#[derive(Debug, Clone, Default)]
pub struct RoleGitView {
    trace: ContainerTrace,
    reader: Option<ReaderPaths>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderPaths {
    pub view: String,
    pub objects: String,
}

impl RoleGitView {
    #[must_use]
    pub fn new(trace: ContainerTrace) -> Self {
        Self {
            trace,
            reader: None,
        }
    }

    #[must_use]
    pub fn for_reader(mut self, view: impl Into<String>, objects: impl Into<String>) -> Self {
        self.reader = Some(ReaderPaths {
            view: view.into(),
            objects: objects.into(),
        });
        self
    }

    #[must_use]
    pub fn reader_paths(&self, request: &GitViewRequest, layout: &GitLayout) -> ReaderPaths {
        self.reader.clone().unwrap_or_else(|| ReaderPaths {
            view: request.path.to_string_lossy().replace('\\', "/"),
            objects: layout.objects.to_string_lossy().replace('\\', "/"),
        })
    }
}

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

    fn discard(&self, path: &Path) -> Result<(), UpstrokeError> {
        // Concurrent reclaimers arbitrate through remove_dir_all: removal and
        // NotFound both let the caller proceed. racing_removal retries other
        // errors, including Windows delete-pending PermissionDenied, up to
        // RACING_ACCESS_ATTEMPTS; persistent failure returns the last error.
        // A protected view must refuse cleanup so its intent remains available
        // for recovery. Every successful call records Discarded, including one
        // that found the view already gone.
        super::racing_removal(path, || fs::remove_dir_all(path))?;
        self.trace.view(ViewAction::Discarded, path);
        Ok(())
    }
}

fn project(
    view: &Path,
    layout: &GitLayout,
    head: &str,
    reader: &ReaderPaths,
) -> Result<(), UpstrokeError> {
    write_file(&view.join("HEAD"), format!("{head}\n").as_bytes())?;

    write_file(&view.join("config"), config_for(layout)?.as_bytes())?;

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

    copy_shared_indexes(&layout.git_dir, view)?;

    create_dir(&view.join("objects").join("info"))?;
    create_dir(&view.join("objects").join("pack"))?;
    write_file(
        &view.join(ALTERNATES),
        format!("{}\n", reader.objects).as_bytes(),
    )?;

    create_dir(&view.join("refs").join("heads"))?;
    create_dir(&view.join("refs").join("tags"))?;

    write_file(
        &view.join(WORKTREE_GITFILE),
        format!("{GITDIR_PREFIX} {}\n", reader.view).as_bytes(),
    )?;
    Ok(())
}

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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(UpstrokeError::Io { path: from, source }),
        };
        write_file(&view.join(&name), &bytes)?;
        copied.push(name);
    }
    copied.sort();
    Ok(copied)
}

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

    pub(crate) fn git(cwd: &Path, args: &[&str]) -> ProcessOutput {
        let mut spec = CommandSpec::new("git");
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

    #[test]
    fn the_view_carries_the_exact_detached_head_and_index_of_the_worktree() {
        let root = scratch("exact");
        let repo = root.join("repo");
        let (head, previous) = repository(&repo);

        for (tag, commit) in [("at-head", &head), ("at-previous", &previous)] {
            let workspace = root.join("tasks").join(tag);
            worktree(&repo, &workspace, commit);
            let layout = resolve(&workspace).expect("resolves").expect("a worktree");

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
            let source_index = std::fs::read(layout.git_dir.join("index")).expect("the index");
            assert!(!source_index.is_empty(), "the fixture staged nothing");
            assert_eq!(
                std::fs::read(view_path.join("index")).expect("the view's index"),
                source_index,
                "{tag}: the index is copied byte for byte"
            );
        }

        assert_ne!(
            std::fs::read_to_string(root.join("views").join("at-head").join("HEAD")).expect("HEAD"),
            std::fs::read_to_string(root.join("views").join("at-previous").join("HEAD"))
                .expect("HEAD"),
        );
    }

    #[test]
    fn a_symbolic_head_is_resolved_to_an_object_id_before_it_reaches_the_view() {
        let root = scratch("symbolic");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let raw = std::fs::read_to_string(repo.join(".git").join("HEAD")).expect("HEAD");
        assert!(
            raw.starts_with("ref:"),
            "the fixture is not symbolic: {raw}"
        );

        let layout = resolve(&repo).expect("resolves").expect("a repository");
        assert_eq!(detached_head(&layout).expect("resolves"), head);

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

        std::fs::write(layout.git_dir.join("HEAD"), "not-an-object-id\n").expect("plant");
        let refusal = detached_head(&layout).expect_err("refuses");
        assert!(
            refusal.to_string().contains("not a Git object id"),
            "{refusal}"
        );
    }

    #[test]
    fn the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository() {
        let root = scratch("no-refs");
        let repo = root.join("repo");
        let (head, _) = repository(&repo);
        let planted = engine_refs(&repo, &head);
        assert_eq!(planted.len(), 3);
        let workspace = root.join("tasks").join("k0-g0");
        worktree(&repo, &workspace, &head);
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

        assert_eq!(WITHHELD_ENTRIES.len(), 4);
        for withheld in WITHHELD_ENTRIES {
            assert!(
                !view_path.join(withheld).exists(),
                "the view carries `{withheld}`, which links back into the repository"
            );
        }
        for dir in ["refs/heads", "refs/tags"] {
            let entries = std::fs::read_dir(view_path.join(dir)).expect(dir).count();
            assert_eq!(entries, 0, "`{dir}` is not empty");
        }
        let config = std::fs::read_to_string(view_path.join("config")).expect("config");
        for forbidden in ["[remote", "url", "credential", "[branch"] {
            assert!(
                !config.contains(forbidden),
                "the view's config names `{forbidden}`: {config}"
            );
        }
        for entry in PROJECTED_ENTRIES {
            assert!(
                view_path.join(entry).exists(),
                "the projection is missing `{entry}`"
            );
        }
    }

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
        assert_eq!(
            through_view(&["status", "--porcelain"]).stdout.trim(),
            "",
            "the index the view carries is not the worktree's"
        );
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
            assert_eq!(
                git_ok(&workspace, &["rev-parse", "--verify", name.as_str()]),
                head
            );
        }

        let after = git_ok(&repo, &["for-each-ref", "--format=%(refname)"]);
        for name in &planted {
            assert!(after.contains(name.as_str()));
        }
    }

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

        for round in 0..2 {
            view.discard(&view_path)
                .unwrap_or_else(|error| panic!("round {round}: {error}"));
            assert!(!view_path.exists());
        }
        assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head);
    }

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
        let kinds: std::collections::BTreeSet<bool> =
            [main.dot_git_is_file, linked_layout.dot_git_is_file]
                .into_iter()
                .collect();
        assert_eq!(kinds.len(), 2);
    }
}

#[cfg(test)]
mod split_index_tests {
    use super::fixtures::{git_ok, repository, scratch, worktree};
    use super::*;

    #[test]
    fn a_split_index_projects_with_the_shared_half_it_links_to() {
        for (label, split) in [("ordinary", false), ("split", true)] {
            let root = scratch(&format!("split-index-{label}"));
            let repo = root.join("repo");
            let (head, _) = repository(&repo);
            let workspace = root.join("tasks").join("k0-g0");
            worktree(&repo, &workspace, &head);

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
