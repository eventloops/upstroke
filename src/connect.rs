//! Extended notes: `docs/internals/connect.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.

#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]

mod render;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::agent::{AdapterSource, BuiltinAdapters, Discovery};
use crate::capacity::{Pool, PoolKind, Source};
use crate::error::UpstrokeError;
use crate::util;

#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    pub pools_path: Option<PathBuf>,

    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    Written,

    Unchanged,

    Refused,
}

#[derive(Debug)]
pub struct ConnectReport {
    pub path: PathBuf,
    pub outcome: Wrote,

    pub content: String,

    pub agents: Vec<AgentReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct AgentReport {
    pub agent: String,

    pub outcome: Result<Discovery, String>,
    pub pool: Option<Pool>,
}

pub fn run(opts: &ConnectOptions) -> Result<ConnectReport, UpstrokeError> {
    run_with(
        opts,
        &BuiltinAdapters,
        crate::agent::ADAPTERS.iter().map(|a| a.id()),
    )
}

pub fn run_with<'a>(
    opts: &ConnectOptions,
    adapters: &dyn AdapterSource,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<ConnectReport, UpstrokeError> {
    let path = match &opts.pools_path {
        Some(path) => path.clone(),
        None => util::user_upstroke_dir()
            .map(|dir| dir.join("pools.toml"))
            .ok_or_else(|| UpstrokeError::Refused {
                message:
                    "cannot find a home directory to write ~/.upstroke/pools.toml into — pass \
                          --pools <path> to say where it should go"
                        .to_owned(),
            })?,
    };

    let existing_text = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => return Err(UpstrokeError::Io { path, source }),
    };
    let mut carried = existing_text
        .as_deref()
        .map(operator_keys)
        .unwrap_or_default();

    let mut warnings = Vec::new();
    let mut agents: Vec<AgentReport> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Some(adapter) = adapters.get(id) else {
            continue;
        };

        let runner = crate::runner::host::HostRunner::new();
        let discovered = adapter
            .probe(&runner)
            .and_then(|caps| adapter.discover(&runner, &caps));
        match discovered {
            Ok(discovery) => {
                let missing = crate::catalog::missing_from(id, &discovery.models);
                if !missing.is_empty() {
                    warnings.push(format!(
                        "{id} does not advertise catalogued model(s): {}. Cross-family review \
                         binds to catalogued names, so one this CLI rejects fails at runtime — \
                         upgrade upstroke or pin a model it lists.",
                        missing.join(", ")
                    ));
                }
                let mut pool = pool_for_agent(id, &discovery);
                if let Some(kept) = carried.remove(&pool.name) {
                    if let Err(error) = kept.apply(&mut pool) {
                        warnings.push(format!("pool `{}`: {error}; using auto", pool.name));
                    }
                }
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Ok(discovery),
                    pool: Some(pool),
                });
            }
            Err(error) => {
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Err(error.to_string()),
                    pool: None,
                });
            }
        }
    }

    let content = render::pools_file(&agents);
    let existing = existing_text;

    let outcome = match &existing {
        Some(existing) if !settings_match(existing, &content) && !opts.force => Wrote::Refused,
        Some(existing)
            if toml::from_str::<toml::Table>(existing).is_ok()
                && stable_content(existing) == stable_content(&content) =>
        {
            Wrote::Unchanged
        }
        _ => {
            write_pools(&path, &content)?;
            Wrote::Written
        }
    };

    Ok(ConnectReport {
        path,
        outcome,
        content,
        agents,
        warnings,
    })
}

fn write_pools(path: &Path, content: &str) -> Result<(), UpstrokeError> {
    publish_pools(path, content, |staged, destination| {
        fs::rename(staged, destination)
    })
}

fn publish_pools(
    path: &Path,
    content: &str,
    publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), UpstrokeError> {
    let Some(named) = publication_directory(path) else {
        return Err(UpstrokeError::Refused {
            message: format!(
                "`{}` is a filesystem root, not a file to write — pass --pools <path> naming \
                 the pools file itself",
                path.display()
            ),
        });
    };
    create_directory_durably(named, util::fsync_dir)?;
    let destination = publication_target(path)?;
    let Some(directory) = publication_directory(&destination) else {
        return Err(UpstrokeError::Refused {
            message: format!(
                "`{}` names a filesystem root, not a file to write",
                destination.display()
            ),
        });
    };
    let inherited = destination_mode(&destination)?;

    let (staged, file) =
        Staged::create(directory.join(format!(".pools-{}.tmp", crate::ulid::ulid())))?;
    let landed = stage(file, staged.path(), content, inherited).and_then(|()| {
        publish(staged.path(), &destination).map_err(|source| UpstrokeError::Filesystem {
            operation: "publish pools file",
            path: destination.clone(),
            source,
        })
    });
    if let Err(error) = landed {
        return Err(staged.withdraw(error));
    }
    staged.published();
    util::fsync_dir(directory).map_err(|source| UpstrokeError::Filesystem {
        operation: "flush the directory of pools file",
        path: directory.to_path_buf(),
        source,
    })
}

fn create_directory_durably(
    directory: &Path,
    mut flush: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<(), UpstrokeError> {
    let mut components: Vec<&Path> = directory
        .ancestors()
        .filter(|ancestor| ancestor.file_name().is_some())
        .collect();
    components.reverse();
    for component in components {
        match fs::create_dir(component) {
            Ok(()) => {
                let Some(parent) = publication_directory(component) else {
                    continue;
                };
                flush(parent).map_err(|source| UpstrokeError::Filesystem {
                    operation: "flush the directory of created directory",
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing =
                    fs::metadata(component).map_err(|source| UpstrokeError::Filesystem {
                        operation: "read the kind of directory",
                        path: component.to_path_buf(),
                        source,
                    })?;
                if !existing.is_dir() {
                    return Err(UpstrokeError::Filesystem {
                        operation: "create directory",
                        path: component.to_path_buf(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "exists and is not a directory",
                        ),
                    });
                }
            }
            Err(source) => {
                return Err(UpstrokeError::Filesystem {
                    operation: "create directory",
                    path: component.to_path_buf(),
                    source,
                });
            }
        }
    }
    Ok(())
}

struct Staged {
    path: PathBuf,
    owned: bool,
}

impl Staged {
    fn create(path: PathBuf) -> Result<(Self, fs::File), UpstrokeError> {
        let file = match fs::File::create_new(&path) {
            Ok(file) => file,
            Err(source) => {
                return Err(UpstrokeError::Filesystem {
                    operation: "create staged pools file",
                    path,
                    source,
                });
            }
        };
        Ok((Staged { path, owned: true }, file))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn published(mut self) {
        self.owned = false;
    }

    fn withdraw(mut self, error: UpstrokeError) -> UpstrokeError {
        self.owned = false;
        let path = std::mem::take(&mut self.path);
        match fs::remove_file(&path) {
            Ok(()) => error,
            Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => error,
            Err(cleanup) => UpstrokeError::Filesystem {
                operation: "remove staged pools file",
                path,
                source: std::io::Error::new(
                    cleanup.kind(),
                    format!("{error}; and the staged file could not be removed: {cleanup}"),
                ),
            },
        }
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => {}
            Err(_cleanup) => {
                // SUPPRESSED, and only here: this arm is reached while the
                // thread is unwinding (see the notes), where a panic would
                // abort the process and cost the diagnosis of whatever
                // actually failed. The destination is intact; what remains
                // is one uniquely named `.tmp` no reader interprets.
            }
        }
    }
}

const SYMLINK_FOLLOW_LIMIT: usize = 40;

fn publication_target(path: &Path) -> Result<PathBuf, UpstrokeError> {
    let mut current = path.to_path_buf();
    let mut followed = 0;
    loop {
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(current);
            }
            Err(source) => {
                return Err(UpstrokeError::Filesystem {
                    operation: "read the kind of pools file",
                    path: current,
                    source,
                });
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(current);
        }
        if followed == SYMLINK_FOLLOW_LIMIT {
            return Err(UpstrokeError::Filesystem {
                operation: "resolve the symlinked pools file",
                path: path.to_path_buf(),
                source: std::io::Error::other(format!(
                    "more than {SYMLINK_FOLLOW_LIMIT} levels of symbolic links"
                )),
            });
        }
        current = named_target(&current)?;
        followed += 1;
    }
}

fn named_target(link: &Path) -> Result<PathBuf, UpstrokeError> {
    let named = fs::read_link(link).map_err(|source| UpstrokeError::Filesystem {
        operation: "read the symlinked pools file",
        path: link.to_path_buf(),
        source,
    })?;
    if named.is_absolute() {
        return Ok(named);
    }
    Ok(match link.parent() {
        Some(parent) => parent.join(named),
        None => named,
    })
}

fn publication_directory(path: &Path) -> Option<&Path> {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Some(Path::new(".")),
        parent => parent,
    }
}

fn destination_mode(path: &Path) -> Result<Option<fs::Permissions>, UpstrokeError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(UpstrokeError::Filesystem {
            operation: "read the mode of pools file",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn stage(
    mut file: fs::File,
    staged: &Path,
    content: &str,
    inherited: Option<fs::Permissions>,
) -> Result<(), UpstrokeError> {
    apply_mode(staged, inherited)?;
    file.write_all(content.as_bytes())
        .map_err(|source| UpstrokeError::Filesystem {
            operation: "write staged pools file",
            path: staged.to_path_buf(),
            source,
        })?;
    util::fsync_file(&file).map_err(|source| UpstrokeError::Filesystem {
        operation: "flush staged pools file",
        path: staged.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn apply_mode(staged: &Path, inherited: Option<fs::Permissions>) -> Result<(), UpstrokeError> {
    let Some(permissions) = inherited else {
        return Ok(());
    };
    fs::set_permissions(staged, permissions).map_err(|source| UpstrokeError::Filesystem {
        operation: "set the mode of staged pools file",
        path: staged.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn apply_mode(_staged: &Path, _inherited: Option<fs::Permissions>) -> Result<(), UpstrokeError> {
    Ok(())
}

fn settings_match(existing: &str, proposed: &str) -> bool {
    match (
        toml::from_str::<toml::Table>(existing),
        toml::from_str::<toml::Table>(proposed),
    ) {
        (Ok(existing), Ok(proposed)) => existing == proposed,
        (Err(_), _) | (_, Err(_)) => false,
    }
}

fn stable_content(text: &str) -> Vec<&str> {
    text.lines()
        .enumerate()
        .filter(|(index, line)| *index != 0 || !line.starts_with(render::WRITTEN_BY))
        .map(|(_, line)| line)
        .collect()
}

fn pool_for_agent(agent: &str, discovery: &Discovery) -> Pool {
    let kind = discovery.shape.unwrap_or(match agent {
        "copilot" => PoolKind::Credits,
        _ => PoolKind::SubscriptionWindow,
    });

    Pool::discovered(
        default_pool_name(agent),
        kind,
        agent,
        vec![Source::Signals, Source::SelfMetered],
    )
}

#[derive(Debug, Default, PartialEq, serde::Deserialize)]
struct OperatorKeys {
    profile: Option<String>,
    monthly_allowance: Option<toml::Value>,
    endpoint: Option<String>,
}

impl OperatorKeys {
    fn apply(self, pool: &mut Pool) -> Result<(), InvalidAllowance> {
        if let Some(profile) = self.profile {
            pool.profile = Some(profile);
        }
        if let Some(endpoint) = self.endpoint {
            pool.endpoint = Some(endpoint);
        }
        if let Some(value) = self.monthly_allowance {
            pool.monthly_allowance = allowance_of(&value)?;
        }
        Ok(())
    }

    fn any(&self) -> bool {
        self.profile.is_some() || self.monthly_allowance.is_some() || self.endpoint.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("monthly_allowance must be a positive finite number or the string \"auto\"")]
struct InvalidAllowance;

fn allowance_of(value: &toml::Value) -> Result<crate::capacity::Allowance, InvalidAllowance> {
    match value {
        toml::Value::String(text) if text.trim().eq_ignore_ascii_case("auto") => {
            Ok(crate::capacity::Allowance::Auto)
        }
        toml::Value::Integer(units) if *units > 0 => {
            Ok(crate::capacity::Allowance::Units(*units as f64))
        }
        toml::Value::Float(units) if units.is_finite() && *units > 0.0 => {
            Ok(crate::capacity::Allowance::Units(*units))
        }
        _ => Err(InvalidAllowance),
    }
}

fn operator_keys(text: &str) -> std::collections::BTreeMap<String, OperatorKeys> {
    #[derive(serde::Deserialize)]
    struct Doc {
        pools: Option<std::collections::BTreeMap<String, OperatorKeys>>,
    }
    toml::from_str::<Doc>(text)
        .ok()
        .and_then(|doc| doc.pools)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, keys)| keys.any())
        .collect()
}

fn default_pool_name(agent: &str) -> &str {
    agent
}

pub fn render_report(report: &ConnectReport) -> String {
    render::report(report)
}

impl ConnectReport {
    pub fn refused(&self) -> bool {
        self.outcome == Wrote::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentAdapter, AuthState, Caps, ProcessOutput, TaskRun};
    use crate::ir::Outcome;
    use crate::runner::CommandSpec;
    use std::path::Path;

    struct FakeAdapter {
        id: &'static str,
        discovery: Option<Discovery>,
    }

    impl AgentAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn probe(&self, _runner: &dyn crate::runner::Runner) -> Result<Caps, UpstrokeError> {
            if self.discovery.is_none() {
                return Err(UpstrokeError::Agent {
                    message: "binary not found on PATH".to_owned(),
                });
            }
            Ok(Caps {
                version: "0.0.0-fake".to_owned(),
                json_output: true,
                session_resume: true,
                cost_reporting: true,
                read_only_mode: true,
                acp: false,
                model_list: false,
            })
        }

        #[expect(
            clippy::unreachable,
            reason = "run_with (this file's only caller of AdapterSource) invokes only probe \
                      and discover on an adapter; build is dead code for this fake by that \
                      local contract"
        )]
        fn build(&self, _run: &TaskRun) -> Result<CommandSpec, UpstrokeError> {
            unreachable!("connect never spawns an attempt")
        }

        #[expect(
            clippy::unreachable,
            reason = "run_with (this file's only caller of AdapterSource) invokes only probe \
                      and discover on an adapter; parse is dead code for this fake by that \
                      local contract"
        )]
        fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, UpstrokeError> {
            unreachable!("connect never parses an attempt")
        }

        fn discover(
            &self,
            _runner: &dyn crate::runner::Runner,
            _caps: &Caps,
        ) -> Result<Discovery, UpstrokeError> {
            self.discovery.clone().ok_or_else(|| UpstrokeError::Agent {
                message: "binary not found on PATH".to_owned(),
            })
        }
    }

    struct Machine {
        adapters: Vec<FakeAdapter>,
    }

    impl AdapterSource for Machine {
        fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
            self.adapters
                .iter()
                .find(|a| a.id == id)
                .map(|a| a as &dyn AgentAdapter)
        }
    }

    fn machine() -> Machine {
        Machine {
            adapters: vec![
                FakeAdapter {
                    id: "claude-code",
                    discovery: Some(Discovery {
                        auth: AuthState::Authenticated,
                        models: Vec::new(),
                        shape: Some(PoolKind::SubscriptionWindow),
                        notes: vec!["auth method `subscription`".to_owned()],
                    }),
                },
                FakeAdapter {
                    id: "copilot",
                    discovery: None,
                },
            ],
        }
    }

    fn connect(path: &Path, force: bool) -> ConnectReport {
        run_with(
            &ConnectOptions {
                pools_path: Some(path.to_path_buf()),
                force,
            },
            &machine(),
            ["claude-code", "copilot"],
        )
        .expect("connect runs")
    }

    /// Every name in `directory` that `publish_pools` could have staged: the
    /// leftovers a publication that does not clean up after itself produces.
    fn staging_files(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("read the pools directory")
            .map(|entry| {
                entry
                    .expect("a directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.starts_with(".pools-") && name.ends_with(".tmp"))
            .collect();
        names.sort();
        names
    }

    /// A publication step that refuses, standing in for the rename failures a
    /// test cannot arrange on demand: a full disk, a kill, a revoked
    /// permission.
    fn refuse_to_publish(_staged: &Path, _destination: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the publication step refused",
        ))
    }

    #[test]
    fn a_failed_publication_leaves_the_operators_file_byte_for_byte_intact() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-failed-publish")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \
                    \"claude-code\"\nprofile = \"work\"\nmonthly_allowance = 300\n";
        fs::write(&path, mine).expect("the operator's hand-written file");

        let error = publish_pools(&path, "a replacement that never lands", refuse_to_publish)
            .expect_err("the publication step refused");

        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "publish pools file", path: ref failed, .. }
            if failed == &path),
            "{error:?}"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read the operator's file"),
            mine,
            "a publication that failed replaced nothing"
        );
        assert_eq!(
            staging_files(tree.path()),
            Vec::<String>::new(),
            "and left no staged file behind"
        );
    }

    #[test]
    fn an_unwinding_publication_leaves_the_operators_file_byte_for_byte_intact() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-unwind-publish")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nprofile = \"work\"\n";
        fs::write(&path, mine).expect("the operator's hand-written file");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            publish_pools(&path, "a replacement that never lands", |_, _| {
                panic!("the failure the operator's file has to survive")
            })
        }));

        assert!(outcome.is_err(), "the fixture must actually unwind");
        assert_eq!(
            fs::read_to_string(&path).expect("read the operator's file"),
            mine,
            "an unwind past a publication replaced nothing"
        );
        assert_eq!(
            staging_files(tree.path()),
            Vec::<String>::new(),
            "and the staged file went with the unwind, not just with a return"
        );
    }

    #[test]
    fn a_publication_flushes_the_staged_file_and_the_directory_it_lands_in() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-barriers")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");

        let before = crate::util::barriers_on_this_thread();
        write_pools(&path, "published once").expect("the publication lands");
        let after = crate::util::barriers_on_this_thread();

        assert_eq!(
            after.file - before.file,
            1,
            "the staged file is flushed before the rename makes it the operator's"
        );
        assert_eq!(
            after.directory - before.directory,
            1,
            "and the directory entry the rename created is flushed after it"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read what was published"),
            "published once"
        );
    }

    #[test]
    fn a_publication_into_directories_it_creates_flushes_each_new_entry_in_its_parent() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-mkdirs")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("made").join("here").join("pools.toml");

        let before = crate::util::barriers_on_this_thread();
        write_pools(&path, "published into a directory that did not exist")
            .expect("the publication lands");
        let after = crate::util::barriers_on_this_thread();

        assert_eq!(after.file - before.file, 1);
        assert_eq!(
            after.directory - before.directory,
            3,
            "two directory entries were created (`made` in the scratch tree, `here` in `made`) \
             and each is flushed in its parent, then `here` is flushed for the published file"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("read what was published"),
            "published into a directory that did not exist"
        );

        let before = crate::util::barriers_on_this_thread();
        write_pools(&path, "published again").expect("the second publication lands");
        let after = crate::util::barriers_on_this_thread();
        assert_eq!(
            after.directory - before.directory,
            1,
            "nothing was created the second time, so only the published file's directory is flushed"
        );
    }

    #[test]
    fn each_created_directory_entry_is_flushed_in_its_parent_outermost_first_and_nothing_else_is() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-mkdir-order")
                .expect("acquire an isolated pools directory");
        let made = tree.path().join("made");
        let here = made.join("here");

        let mut flushed: Vec<PathBuf> = Vec::new();
        create_directory_durably(&here, |parent| {
            flushed.push(parent.to_path_buf());
            Ok(())
        })
        .expect("the directories are created");

        assert_eq!(
            flushed,
            vec![tree.path().to_path_buf(), made.clone()],
            "the entry `made` is flushed in the scratch tree, then `here` in `made`, in that \
             order; the scratch tree's own ancestors already existed and are not this call's to \
             flush"
        );
        assert!(here.is_dir());

        flushed.clear();
        create_directory_durably(&here, |parent| {
            flushed.push(parent.to_path_buf());
            Ok(())
        })
        .expect("an existing directory is accepted");
        assert_eq!(
            flushed,
            Vec::<PathBuf>::new(),
            "nothing was created the second time, so nothing is flushed"
        );
    }

    #[test]
    fn a_directory_removed_by_someone_else_between_two_creations_is_a_reported_failure() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-mkdir-removed")
                .expect("acquire an isolated pools directory");
        let made = tree.path().join("made");
        let here = made.join("here");

        let error = create_directory_durably(&here, |parent| {
            if parent == tree.path() {
                fs::remove_dir(&made).expect("someone else removes `made` right after it is made");
            }
            Ok(())
        })
        .expect_err("`here` cannot be created inside a directory that is gone");

        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "create directory", path: ref failed, ref source }
            if failed == &here && source.kind() == std::io::ErrorKind::NotFound),
            "{error:?}"
        );
        assert!(
            !made.exists(),
            "nothing recreated what someone else removed"
        );
    }

    #[test]
    fn a_directory_replaced_by_someone_else_between_two_creations_is_used_and_only_this_calls_entries_are_flushed()
     {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-mkdir-replaced")
                .expect("acquire an isolated pools directory");
        let made = tree.path().join("made");
        let here = made.join("here");

        let mut flushed: Vec<PathBuf> = Vec::new();
        create_directory_durably(&here, |parent| {
            if parent == tree.path() {
                fs::remove_dir(&made).expect("someone else removes `made`");
                fs::create_dir(&made).expect("and puts their own `made` in its place");
            }
            flushed.push(parent.to_path_buf());
            Ok(())
        })
        .expect("`here` is created inside the directory someone else now owns");

        assert!(here.is_dir());
        assert_eq!(
            flushed,
            vec![tree.path().to_path_buf(), made.clone()],
            "this call flushed the `made` it created and the `here` it created; the `made` \
             someone else recreated is theirs to flush, and `create_dir` said so by refusing \
             to create it twice"
        );
    }

    #[test]
    fn the_directory_a_pools_file_is_published_into_is_openable() {
        assert_eq!(
            publication_directory(Path::new("pools.toml")),
            Some(Path::new(".")),
            "a bare relative name is published into the working directory, which `\"\"` does \
             not name"
        );
        assert_eq!(
            publication_directory(Path::new("dir/pools.toml")),
            Some(Path::new("dir"))
        );
        assert_eq!(
            publication_directory(Path::new(std::path::MAIN_SEPARATOR_STR)),
            None,
            "a filesystem root names no file to publish"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_pools_file_is_published_through_the_link_rather_than_over_it() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-symlink")
            .expect("acquire an isolated pools directory");
        let real = tree.path().join("dotfiles-pools.toml");
        let link = tree.path().join("pools.toml");
        fs::write(&real, "[pools.claude-code]\nprofile = \"work\"\n")
            .expect("the file the operator keeps their configuration in");
        std::os::unix::fs::symlink(&real, &link).expect("the operator's dotfile link");

        assert_eq!(connect(&link, true).outcome, Wrote::Written);

        assert!(
            fs::symlink_metadata(&link)
                .expect("read the link")
                .file_type()
                .is_symlink(),
            "publishing replaced the operator's symlink with a regular file"
        );
        let published = fs::read_to_string(&real).expect("read the file the link names");
        assert!(
            published.contains("[pools.claude-code]") && published.contains("weekly = true"),
            "the file the link names is what was rewritten:\n{published}"
        );
        assert_eq!(
            fs::read_to_string(&link).expect("read through the link"),
            published,
            "and the link still reaches it"
        );
        assert_eq!(staging_files(tree.path()), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_pools_symlink_publishes_the_file_it_names() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-dangling")
            .expect("acquire an isolated pools directory");
        let named = tree.path().join("not-there-yet.toml");
        let link = tree.path().join("pools.toml");
        std::os::unix::fs::symlink("not-there-yet.toml", &link)
            .expect("a link to a file that does not exist yet");

        assert_eq!(connect(&link, false).outcome, Wrote::Written);

        assert!(
            fs::symlink_metadata(&link)
                .expect("read the link")
                .file_type()
                .is_symlink(),
            "a dangling link is still the operator's link"
        );
        assert!(
            fs::read_to_string(&named)
                .expect("the file the link names now exists")
                .contains("[pools.claude-code]")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_chain_of_pools_symlinks_publishes_the_file_the_last_link_names() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-chain")
            .expect("acquire an isolated pools directory");
        let dotfiles = tree.path().join("dotfiles");
        fs::create_dir(&dotfiles).expect("the directory the intermediate link lives in");
        let link = tree.path().join("pools.toml");
        let intermediate = dotfiles.join("intermediate.toml");
        let named = tree.path().join("absent-final.toml");
        std::os::unix::fs::symlink("dotfiles/intermediate.toml", &link)
            .expect("the operator's link, to another link");
        std::os::unix::fs::symlink("../absent-final.toml", &intermediate)
            .expect("a relative link that only resolves against its own directory");

        assert_eq!(connect(&link, false).outcome, Wrote::Written);

        assert!(
            fs::symlink_metadata(&link)
                .expect("read the link")
                .file_type()
                .is_symlink(),
            "the operator's link is still a link"
        );
        assert!(
            fs::symlink_metadata(&intermediate)
                .expect("read the intermediate link")
                .file_type()
                .is_symlink(),
            "and so is the link it names: a one-level resolution publishes over this one"
        );
        let published = fs::read_to_string(&named).expect("the file the chain names now exists");
        assert!(published.contains("[pools.claude-code]"), "{published}");
        assert_eq!(
            fs::read_to_string(&link).expect("read through the chain"),
            published,
            "and the chain still reaches it"
        );
        assert_eq!(staging_files(tree.path()), Vec::<String>::new());
    }

    /// `count` links in a row, `pools.toml -> link-1 -> … -> link-<count-1>
    /// -> final.toml`, every one relative; returns the first link and the
    /// file the chain names, which does not exist.
    #[cfg(unix)]
    fn chain_of(tree: &Path, count: usize) -> (PathBuf, PathBuf) {
        let name = |index: usize| {
            if index == 0 {
                "pools.toml".to_owned()
            } else {
                format!("link-{index}.toml")
            }
        };
        for index in 0..count {
            let target = if index + 1 == count {
                "final.toml".to_owned()
            } else {
                name(index + 1)
            };
            std::os::unix::fs::symlink(&target, tree.join(name(index)))
                .expect("one link of the chain");
        }
        (tree.join(name(0)), tree.join("final.toml"))
    }

    #[cfg(unix)]
    #[test]
    fn a_chain_at_the_follow_limit_is_published_and_one_past_it_is_refused() {
        for count in [SYMLINK_FOLLOW_LIMIT - 1, SYMLINK_FOLLOW_LIMIT] {
            let tree =
                crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-chain-limit")
                    .expect("acquire an isolated pools directory");
            let (link, named) = chain_of(tree.path(), count);

            write_pools(&link, "published through the chain").expect("a chain the bound admits");

            assert_eq!(
                fs::read_to_string(&named).expect("the file the chain names now exists"),
                "published through the chain",
                "{count} links"
            );
            assert!(
                fs::symlink_metadata(tree.path().join(format!("link-{}.toml", count - 1)))
                    .expect("read the last link")
                    .file_type()
                    .is_symlink(),
                "the last of {count} links is still a link"
            );
            assert_eq!(staging_files(tree.path()), Vec::<String>::new());
        }

        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-chain-over")
                .expect("acquire an isolated pools directory");
        let (link, named) = chain_of(tree.path(), SYMLINK_FOLLOW_LIMIT + 1);

        let error = write_pools(&link, "a replacement with no file to land on")
            .expect_err("one link past the bound is refused");

        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "resolve the symlinked pools file", path: ref failed, ref source }
            if failed == &link && source.to_string().contains("levels of symbolic links")),
            "{error:?}"
        );
        assert!(!named.exists(), "nothing was published past the bound");
        assert_eq!(
            staging_files(tree.path()),
            Vec::<String>::new(),
            "nothing was staged for a publication with no destination"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_mode_an_operator_gave_their_pools_file_survives_a_forced_rewrite() {
        use std::os::unix::fs::PermissionsExt as _;

        const OPERATORS: u32 = 0o740;

        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-mode")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \
                    \"claude-code\"\nprofile = \"work\"\n";
        fs::write(&path, mine).expect("the operator's hand-written file");
        fs::set_permissions(&path, fs::Permissions::from_mode(OPERATORS))
            .expect("the operator restricts their own file");

        assert_eq!(connect(&path, true).outcome, Wrote::Written);

        let mode = fs::metadata(&path)
            .expect("read the mode of the rewritten file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, OPERATORS,
            "a rewrite that publishes a new inode must not hand back the mode the umask \
             gives a fresh file in place of the one the operator set"
        );
    }

    #[test]
    fn quoted_pool_renames_preserve_every_string_byte_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-quoted")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "x y",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let options = ConnectOptions {
            pools_path: Some(path.clone()),
            force: false,
        };
        let first = run_with(&options, &machine, ["x y"]).expect("generate a quoted pool key");
        assert_eq!(first.outcome, Wrote::Written);
        for header in [
            "[pools.\"x  y\"]",
            "[pools.\"x\ty\"]",
            "[pools.\"x\u{a0}y\"]",
            "[pools.'x  y']",
            "[pools.'x\ty']",
            "[pools.\" x y\"]",
            "[pools.\"x y \"]",
        ] {
            let renamed = first.content.replace("[pools.\"x y\"]", header);
            assert_ne!(renamed, first.content, "the header replacement must apply");
            let parsed: toml::Table = toml::from_str(&renamed).expect("a valid TOML rename");
            assert!(parsed.get("pools").is_some(), "the renamed pool exists");
            fs::write(&path, &renamed).expect("write the operator's rename");
            let next = run_with(&options, &machine, ["x y"]).expect("compare the renamed pool");
            assert_eq!(next.outcome, Wrote::Refused, "header {header:?}");
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                renamed
            );
        }
    }

    #[test]
    fn review_168_force_replaces_a_malformed_generated_header() {
        let tree = crate::rundir::scratch_tree::acquire(
            &std::env::temp_dir(),
            "review-168-malformed-header",
        )
        .expect("acquire isolated review fixture");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let (header, rest) = first
            .content
            .split_once('\n')
            .expect("generated file has a first header line");
        let malformed = format!("{header}\u{1}\n{rest}");
        assert!(toml::from_str::<toml::Table>(&malformed).is_err());
        fs::write(&path, &malformed).expect("corrupt only the generated header");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(
            fs::read_to_string(&path).expect("read refusal bytes"),
            malformed
        );

        let forced = connect(&path, true);
        let persisted = fs::read_to_string(&path).expect("read forced result");
        let mut warnings = Vec::new();
        let loaded = crate::config::load(None, tree.path(), Some(&path), &mut warnings);
        assert_eq!(
            forced.outcome,
            Wrote::Written,
            "--force must repair malformed TOML; original bytes retained: {}; config reader: {loaded:?}",
            persisted == malformed,
        );
        assert!(loaded.is_ok(), "the repaired file must load: {loaded:?}");
        assert_ne!(persisted, malformed);
    }

    #[test]
    fn equivalent_toml_spellings_refresh_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-spelling")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        for (from, to) in [
            ("reserve = 0.20", "reserve = 0.2"),
            ("[pools.claude-code]", "[pools.'claude-code']"),
            ("[\"signals\", \"self\"]", "[ 'signals' ,\n 'self',\n]"),
            ("agent = \"claude-code\"", "agent = '''claude-code'''"),
        ] {
            let edited = first.content.replace(from, to);
            assert_ne!(edited, first.content, "the spelling change must apply");
            fs::write(&path, edited).expect("write an equivalent TOML spelling");
            assert_eq!(connect(&path, false).outcome, Wrote::Written, "{to}");
            assert_eq!(connect(&path, false).outcome, Wrote::Unchanged, "{to}");
        }
    }

    #[test]
    fn a_changed_multiline_string_is_refused_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-multiline")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        for spelling in ["'''claude-\ncode'''", "\"\"\"claude-\ncode\"\"\""] {
            let edited = first
                .content
                .replace("agent = \"claude-code\"", &format!("agent = {spelling}"));
            assert_ne!(edited, first.content, "the multiline change must apply");
            toml::from_str::<toml::Table>(&edited).expect("valid multiline TOML string");
            fs::write(&path, &edited).expect("write a changed multiline string");
            assert_eq!(connect(&path, false).outcome, Wrote::Refused);
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                edited
            );
        }
    }

    #[test]
    fn an_old_integer_allowance_requires_force_once_when_the_writer_uses_a_float() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-integer")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);

        let old = first.content.replace(
            "agent = \"claude-code\"",
            "agent = \"claude-code\"\nmonthly_allowance = 10000000000000000",
        );
        assert_ne!(old, first.content, "insert the previous allowance spelling");
        fs::write(&path, &old).expect("write the old renderer's integer spelling");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(fs::read_to_string(&path).expect("read after refusal"), old);
        assert_eq!(connect(&path, true).outcome, Wrote::Written);
        let mut warnings = Vec::new();
        let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
            .expect("the real config reader accepts the rewritten allowance");
        assert_eq!(
            config.pools.first().expect("one pool").monthly_allowance,
            crate::capacity::Allowance::Units(1e16)
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(connect(&path, false).outcome, Wrote::Unchanged);
    }

    #[test]
    fn force_rejects_invalid_allowances_and_preserves_other_operator_keys() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-allowance")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        for allowance in [
            "nan", "inf", "-inf", "0", "-1", "0.0", "-0.0", "-1.5", "true", "'bad'",
        ] {
            let existing = format!(
                "[pools.claude-code]\nkind = 'subscription-window'\nagent = 'claude-code'\n\
                 profile = 'work'\nendpoint = 'http://host/#work'\nmonthly_allowance = {allowance}\n"
            );
            fs::write(&path, &existing).expect("write an invalid allowance with other valid keys");
            let mut warnings = Vec::new();
            assert!(
                crate::config::load(None, tree.path(), Some(&path), &mut warnings).is_err(),
                "the real reader rejects {allowance}"
            );
            assert_eq!(connect(&path, false).outcome, Wrote::Refused);
            assert_eq!(
                fs::read_to_string(&path).expect("read after refusal"),
                existing
            );
            let forced = connect(&path, true);
            assert_eq!(forced.outcome, Wrote::Written);
            let mut warnings = Vec::new();
            let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
                .expect("the real reader accepts what --force wrote");
            let pool = config.pools.first().expect("one pool");
            assert_eq!(pool.monthly_allowance, crate::capacity::Allowance::Auto);
            assert_eq!(pool.profile.as_deref(), Some("work"));
            assert_eq!(pool.endpoint.as_deref(), Some("http://host/#work"));
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(
                forced
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("monthly_allowance")),
                "the rejection of {allowance} is visible: {:?}",
                forced.warnings
            );
        }
    }

    #[test]
    fn force_keeps_positive_allowances_the_config_reader_accepts() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-valid-allowance")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        for (spelling, expected) in [
            ("1e-300", 1e-300),
            ("0.5", 0.5),
            ("300", 300.0),
            ("1e16", 1e16),
            ("1e300", 1e300),
        ] {
            fs::write(
                &path,
                format!("[pools.claude-code]\nmonthly_allowance = {spelling}\n"),
            )
            .expect("write a valid allowance");
            let report = connect(&path, true);
            assert_eq!(report.outcome, Wrote::Written);
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            let mut warnings = Vec::new();
            let config = crate::config::load(None, tree.path(), Some(&path), &mut warnings)
                .expect("the real reader accepts the carried allowance");
            assert_eq!(
                config.pools.first().expect("one pool").monthly_allowance,
                crate::capacity::Allowance::Units(expected),
                "{spelling}"
            );
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn invalid_toml_in_a_comment_is_refused_without_force() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-comment")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let invalid = format!("# operator note with \u{1} control\n{}", first.content);
        assert!(toml::from_str::<toml::Table>(&invalid).is_err());
        fs::write(&path, &invalid).expect("write a malformed comment");
        assert_eq!(connect(&path, false).outcome, Wrote::Refused);
        assert_eq!(
            fs::read_to_string(&path).expect("read after refusal"),
            invalid
        );
        assert_eq!(connect(&path, true).outcome, Wrote::Written);
    }

    #[test]
    fn a_different_header_timestamp_is_unchanged_and_keeps_the_existing_bytes() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-timestamp")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);
        let old = render::pools_file_at(&first.agents, "1900-01-01T00:00:00Z");
        assert_ne!(old, first.content, "the timestamp must differ");
        fs::write(&path, &old).expect("write a controlled older timestamp");
        assert_eq!(connect(&path, false).outcome, Wrote::Unchanged);
        assert_eq!(fs::read_to_string(&path).expect("read unchanged file"), old);
    }

    #[test]
    fn the_timestamp_prefix_on_a_later_comment_does_not_hide_a_content_change() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-later-prefix")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let first = connect(&path, false);

        let annotated = format!("{}{} operator note\n", first.content, render::WRITTEN_BY);
        fs::write(&path, annotated).expect("append a comment using the header prefix");
        assert_eq!(connect(&path, false).outcome, Wrote::Written);
    }

    #[test]
    fn an_unreadable_existing_file_is_an_error_even_with_force() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-read-error")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        fs::write(&path, [0xff, 0xfe]).expect("write a file that cannot be read as UTF-8");
        for force in [false, true] {
            let error = run_with(
                &ConnectOptions {
                    pools_path: Some(path.clone()),
                    force,
                },
                &machine(),
                ["claude-code"],
            )
            .expect_err("an unreadable file must not become absence");
            assert!(
                matches!(error, UpstrokeError::Io { path: ref failed, ref source }
                if failed == &path && source.kind() == std::io::ErrorKind::InvalidData)
            );
            assert_eq!(fs::read(&path).expect("read original bytes"), [0xff, 0xfe]);
        }
    }

    #[test]
    fn pools_write_error_names_a_failed_parent_creation() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-create-error")
                .expect("acquire an isolated pools directory");
        let parent = tree.path().join("not-a-directory");
        fs::write(&parent, "original").expect("block directory creation with a file");
        let error = write_pools(&parent.join("pools.toml"), "proposal")
            .expect_err("the parent path is a file");
        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "create directory", path, .. }
            if path == parent)
        );
        assert_eq!(
            fs::read_to_string(&parent).expect("read the original file"),
            "original"
        );
    }

    #[test]
    fn a_real_publication_failure_names_the_destination_and_removes_the_staged_file() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-write-error")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("is-a-directory");
        fs::create_dir(&path).expect("block publication with a directory");
        let error = write_pools(&path, "proposal").expect_err("the file path is a directory");
        assert!(
            matches!(error, UpstrokeError::Filesystem { operation: "publish pools file", path: ref failed, .. }
            if failed == &path),
            "{error:?}"
        );
        assert!(path.is_dir());
        assert_eq!(
            staging_files(tree.path()),
            Vec::<String>::new(),
            "the real `fs::rename` failed and its staged file was removed"
        );
    }

    #[test]
    fn discovery_note_lines_cannot_impersonate_warning_records() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-note")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "claude-code",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: vec![
                        "odd\nwarning: forged\r\n\u{1b}[2Kcontrol\rcarriage\u{85}next".to_owned(),
                    ],
                }),
            }],
        };
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(path),
                force: false,
            },
            &machine,
            ["claude-code"],
        )
        .expect("connect with an untrusted discovery note");
        let output = render_report(&report);
        assert!(output.contains("\n  odd\n  warning: forged\n"), "{output}");
        assert!(
            !output.lines().any(|line| line.starts_with("warning:")),
            "{output}"
        );
        assert!(
            output
                .lines()
                .all(|line| !line.chars().any(char::is_control)),
            "{output:?}"
        );
    }

    #[test]
    fn a_skipped_agent_is_reported_once() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-skipped")
            .expect("acquire an isolated pools directory");
        let report = connect(&tree.path().join("pools.toml"), false);
        let output = render_report(&report);
        assert_eq!(
            output.matches("binary not found on PATH").count(),
            1,
            "{output}"
        );
        assert!(
            output
                .lines()
                .any(|line| line.starts_with("copilot: skipped")),
            "{output}"
        );
    }

    #[test]
    fn a_missing_agent_skips_its_pool_without_taking_the_others_with_it() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-partial")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Written);
        let written = fs::read_to_string(&path).expect("file");
        assert!(written.contains("[pools.claude-code]"), "{written}");
        assert!(
            !written.contains("[pools.copilot]"),
            "no pool for a CLI that is not installed: {written}"
        );
        assert!(
            written.contains("# copilot: not usable"),
            "and it says why: {written}"
        );
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn what_connect_writes_parses_back_into_the_pools_it_describes() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-roundtrip")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        connect(&path, false);
        let mut warnings = Vec::new();
        let hermetic = path.parent().expect("parent").to_path_buf();
        let cfg = crate::config::load(None, &hermetic, Some(&path), &mut warnings)
            .expect("the written file parses");
        assert_eq!(cfg.pools.len(), 1);
        let pool = &cfg.pools[0];
        assert_eq!(pool.name, "claude-code");
        assert_eq!(pool.kind, PoolKind::SubscriptionWindow);
        assert_eq!(pool.agent, "claude-code");
        assert_eq!(pool.sources, [Source::Signals, Source::SelfMetered]);
        assert_eq!(pool.safety_margin, crate::capacity::DEFAULT_SAFETY_MARGIN);
        assert_eq!(pool.reserve, crate::capacity::DEFAULT_RESERVE);
        assert_eq!(pool.profile, None, "connect never invents a profile");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn an_existing_file_that_differs_is_never_clobbered() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-clobber")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \
                    \"claude-code\"\nprofile = \"work\"\nmonthly_allowance = 300\n";
        fs::write(&path, mine).expect("hand-written file");

        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Refused);
        assert!(report.refused());
        assert_eq!(
            fs::read_to_string(&path).expect("still there"),
            mine,
            "the hand-written file is untouched"
        );
        let rendered = render_report(&report);
        assert!(rendered.contains("--force"), "{rendered}");
        assert!(
            rendered.contains("[pools.claude-code]"),
            "it shows what it would have written: {rendered}"
        );

        let forced = connect(&path, true);
        assert_eq!(forced.outcome, Wrote::Written);
        let after = fs::read_to_string(&path).expect("file");
        assert!(
            after.contains("profile = \"work\"") && after.contains("monthly_allowance = 300"),
            "--force keeps operator keys:
{after}"
        );
        assert!(
            after.contains("weekly = true"),
            "and still refreshes the rest:
{after}"
        );
    }

    #[test]
    fn operator_keys_with_toml_escapes_survive_a_force_and_read_back_unchanged() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-escapes")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n\
                    profile = 'C:\\Users\\me\\.claude-work'\nendpoint = \"http://host/#frag \\\"q\\\"\"\n";
        fs::write(&path, mine).expect("hand-written file");

        let forced = connect(&path, true);
        assert_eq!(forced.outcome, Wrote::Written);
        let mut warnings = Vec::new();
        let hermetic = path.parent().expect("parent").to_path_buf();
        let cfg = crate::config::load(None, &hermetic, Some(&path), &mut warnings)
            .expect("the file --force wrote parses");
        let pool = cfg.pools.first().expect("one pool");
        assert_eq!(pool.profile.as_deref(), Some(r"C:\Users\me\.claude-work"));
        assert_eq!(pool.endpoint.as_deref(), Some(r#"http://host/#frag "q""#));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        let again = connect(&path, false);
        assert_eq!(again.outcome, Wrote::Unchanged, "{}", again.content);
        assert!(
            again
                .content
                .contains("profile = \"C:\\\\Users\\\\me\\\\.claude-work\""),
            "{}",
            again.content
        );
    }

    #[test]
    fn a_renamed_pool_whose_key_carries_an_escaped_quote_is_refused_not_overwritten() {
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "x\"#A",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    models: Vec::new(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-escaped-key")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let opts = ConnectOptions {
            pools_path: Some(path.clone()),
            force: false,
        };
        let first = run_with(&opts, &machine, ["x\"#A"]).expect("first connect");
        assert_eq!(first.outcome, Wrote::Written);
        let written = fs::read_to_string(&path).expect("file");
        assert!(
            written.contains("[pools.\"x\\\"#A\"]"),
            "the key is quoted and escaped: {written}"
        );

        let renamed = written.replace("[pools.\"x\\\"#A\"]", "[pools.\"x\\\"#B\"]");
        assert_ne!(renamed, written, "the rename changed the file");
        fs::write(&path, &renamed).expect("renamed by hand");
        let second = run_with(&opts, &machine, ["x\"#A"]).expect("second connect");
        assert_eq!(
            second.outcome,
            Wrote::Refused,
            "a renamed pool is a settings difference"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            renamed,
            "the operator's rename is untouched"
        );
    }

    #[test]
    fn a_file_connect_cannot_parse_carries_nothing_and_the_refusal_does_not_claim_otherwise() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-unparseable")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let mine = "[pools.claude-code]\nkind = \"subscription-window\"\nagent = \"claude-code\"\n\
                    profile = \"work\"\nthis line is not toml\n";
        fs::write(&path, mine).expect("hand-written file");
        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Refused);
        assert!(
            !report.content.contains("profile = "),
            "nothing was carried out of a file that does not parse:\n{}",
            report.content
        );
        let rendered = render_report(&report);
        assert!(
            !rendered.contains("profile = \"work\""),
            "the proposed text shows the profile is gone: {rendered}"
        );
        assert!(
            rendered.contains("only when it can parse the existing file"),
            "the refusal says when keys are carried, not that they were: {rendered}"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            mine,
            "and nothing was written"
        );
    }

    #[test]
    fn re_connecting_an_unchanged_machine_reports_unchanged_rather_than_a_conflict() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-idempotent")
                .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        connect(&path, false);
        let first = fs::read_to_string(&path).expect("file");

        let again = connect(&path, false);
        assert_eq!(again.outcome, Wrote::Unchanged, "{:?}", again.outcome);
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            first,
            "nothing changed, so nothing was rewritten — including the date it says it was written"
        );

        fs::write(&path, format!("# my own note\n{first}")).expect("annotate");
        assert_eq!(connect(&path, false).outcome, Wrote::Written);
    }

    #[test]
    fn a_login_between_connects_updates_the_file() {
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-relogin")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let with = |auth: AuthState| Machine {
            adapters: vec![FakeAdapter {
                id: "claude-code",
                discovery: Some(Discovery {
                    auth,
                    models: Vec::new(),
                    shape: Some(PoolKind::SubscriptionWindow),
                    notes: Vec::new(),
                }),
            }],
        };
        let opts = |force| ConnectOptions {
            pools_path: Some(path.clone()),
            force,
        };
        run_with(
            &opts(false),
            &with(AuthState::NotAuthenticated),
            ["claude-code"],
        )
        .expect("first connect");
        assert!(
            fs::read_to_string(&path)
                .expect("file")
                .contains("NOT signed in"),
            "precondition"
        );

        let second = run_with(
            &opts(false),
            &with(AuthState::Authenticated),
            ["claude-code"],
        )
        .expect("second connect");
        assert_eq!(second.outcome, Wrote::Written, "the auth state changed");
        assert!(
            !fs::read_to_string(&path)
                .expect("file")
                .contains("NOT signed in"),
            "the file must not still say the operator is signed out:\n{}",
            fs::read_to_string(&path).expect("file")
        );
    }

    #[test]
    fn a_cli_that_lists_models_is_cross_checked_against_the_catalog() {
        let tree =
            crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-crosscheck")
                .expect("acquire an isolated pools directory");
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,

                    models: [
                        "gpt-5-mini",
                        "gemini-3.1-pro",
                        "claude-sonnet-5",
                        "claude-opus-5",
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(tree.path().join("pools.toml")),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        let warning = report
            .warnings
            .iter()
            .find(|w| w.contains("does not advertise"))
            .unwrap_or_else(|| panic!("expected a cross-check warning: {:?}", report.warnings));
        assert!(
            warning.contains("gpt-5.3-codex"),
            "names the frontier slug the second opinion depends on: {warning}"
        );
    }

    #[test]
    fn an_undetectable_plan_shape_takes_a_default_and_says_so() {
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery::unknown().with_note("no auth query exists")),
            }],
        };
        let tree = crate::rundir::scratch_tree::acquire(&std::env::temp_dir(), "connect-shape")
            .expect("acquire an isolated pools directory");
        let path = tree.path().join("pools.toml");
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(path),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        assert!(
            report.content.contains("kind = \"credits\""),
            "{}",
            report.content
        );
        assert!(
            report.content.contains("kind below is a default"),
            "the default is visible in the file: {}",
            report.content
        );
        assert!(
            report
                .content
                .contains("auth state could not be determined"),
            "unknown auth never renders as 'not connected': {}",
            report.content
        );
    }

    #[test]
    fn discovery_against_the_real_claude_binary_when_present() {
        let runner = crate::runner::host::HostRunner::new();
        let Ok(caps) = crate::agent::claude::ClaudeCodeAdapter.probe(&runner) else {
            eprintln!("skipped: no claude on PATH");
            return;
        };
        let discovery = crate::agent::claude::ClaudeCodeAdapter
            .discover(&runner, &caps)
            .expect("discovery never fails on a CLI that probes");

        assert!(
            !discovery.notes.is_empty(),
            "discovery always says how it worked it out"
        );
        assert!(
            discovery.models.is_empty() || caps.model_list,
            "models may only be reported by a CLI whose --help advertises listing"
        );
    }
}
