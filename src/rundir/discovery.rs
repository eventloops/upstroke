//! Run discovery: the readers `startup_census` names, and the husk report.
//!
//! `startup_census`: "every reader (`list_runs`, `latest_run`,
//! `resolve_run_id`, `find_question`, `status`) returns Committed directories
//! only, **whether or not a marker is present**". Every function here is one of
//! those readers or the census surface `upstroke status` renders, and every one
//! of them is read-only: the census *decides* here and *acts* in the parent,
//! where the deletion sites are.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree and not by the file, so an out-of-line
// child of `src/rundir.rs` would otherwise inherit that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// -- `PR6-LANEF-004`, measured twice in the Container subtree and made again,
// independently, by two W1 pull requests. Nothing here reaches a governed
// primitive, so all three are DENIED rather than allowed, and this module takes
// no `effects/allowlist.toml` row: an allowance is what that file records, and
// this module takes none.
//
// **Measured, not believed.** A probe of three lines -- a `std::fs::write`, a
// `std::process::Command` and a `println!` -- is refused three times here, once
// per lint, with this attribute cited as the level; the identical three lines in
// `src/rundir.rs` emit no `disallowed_*` at all, under that file's own allow. So
// the deny is load-bearing rather than a restatement of an ambient rule.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::UpstrokeError;

use super::{
    CreatingMarker, MARKER, PrivateHalfOwnership, RepoKey, RetainReason, RunDirClass, UnboundShape,
    classify_run_dir, fs, prove_private_half_ownership, public_dir, runs_root,
};

/// Every run in this repo, oldest first.
///
/// Run ids are ULIDs with the millisecond timestamp in the high bits and
/// Crockford base32's digits-before-letters ordering, so a plain lexicographic
/// sort is chronological — no directory timestamps, which copying a repo would
/// scramble.
///
/// **Committed directories only.** `startup_census`: "every reader
/// (`list_runs`, `latest_run`, `resolve_run_id`, `find_question`, `status`)
/// returns Committed directories only, **whether or not a marker is present**",
/// and `run_creation` says it from the other side: "readers never return a
/// directory without a committed `run_started` and never hide one because of a
/// marker". Both halves are load-bearing and each is a separate test.
///
/// An incomplete enumeration returns its operation, path, and cause instead
/// of an empty or partial answer.
///
/// # Errors
///
/// Returns the path and cause when the run-directory enumeration is incomplete.
pub fn list_runs(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {
    // The enumeration already names the failed operation and path.
    let runs = run_ids(repo_root)?
        .into_iter()
        .filter(|run_id| classify_run_dir(&public_dir(repo_root, run_id)) == RunDirClass::Committed)
        .collect();
    Ok(runs)
}

/// Every directory under `<repo>/.upstroke/runs`, **as the filesystem spells
/// it**, committed or not, oldest first — or the failure that stopped the
/// listing.
///
/// Not a reader in `startup_census`'s sense and deliberately not filtered by
/// commitment: this is the enumeration a census walks and the one the worktree
/// lease's R28 check scans. A crashed run whose log never committed is exactly
/// the run whose reaper is most likely still holding its cleanup lease, so
/// filtering here would hide the hold that check exists to observe.
///
/// **The name is taken exactly or not at all.** This mapped each entry through
/// `to_string_lossy()` and `engine::topology::startup::scan` rebuilds a path
/// from the result, so a directory named with the bytes `x\xff` was enumerated
/// as `x` + `U+FFFD`: the census then inspected a directory that does not
/// exist while the real one was never inspected and never reported, and where
/// both names existed the valid one was scanned twice and the other not at all.
/// A lossy name is a diagnostic and never an identity, and this one is handed
/// to a census that deletes.
///
/// **A listing that did not happen is not an empty runs directory**, and this
/// was the last place in the reclaim family that said it was. It opened the
/// directory with `let Ok(entries) = … else { return Vec::new() }`, dropped
/// per-entry errors with `flatten()`, and dropped names that are not valid
/// UTF-8 — three folds of the class `SWEEP-CLASSIFY-009` names, in the function
/// whose own doc says hiding a directory defeats R28. Review pass 5 of PR #139
/// wrote the harm down: a killed conductor's reaper holds the cleanup lease,
/// the runs directory is briefly unlistable, this answered "no runs", and the
/// next coordinator's lease probe never looked — overlapping engine ownership
/// while the reaper was still active.
///
/// So the answer is a `Result` of exact names and every caller decides. The
/// two that decide *ownership* — the worktree lease's R28 probe and the startup
/// census — treat an error as an error: refuse, or fail the command. The
/// readers (`list_runs`, `list_husks`, `husk_matching`) go through
/// [`run_ids`] for UTF-8 identifiers and preserve enumeration failures too.
///
/// # Errors
///
/// Opening, iterating, or inspecting an entry failed. An absent runs root is
/// an empty repository, but an error after enumeration began never becomes an
/// empty or partial answer, including an entry that disappeared during the walk.
pub fn run_dir_names(repo_root: &Path) -> Result<Vec<OsString>, UpstrokeError> {
    let root = runs_root(repo_root);
    let Some(entries) = read_directory_if_present(&root)? else {
        return Ok(Vec::new());
    };
    collect_run_directories(
        &root,
        entries.map(|entry| entry.map(|entry| (entry.file_name(), entry.path()))),
    )
}

/// Only an absent directory name answers `None`. Failure to open a dangling
/// link or to inspect the name remains an error with its operation and path.
fn read_directory_if_present(root: &Path) -> Result<Option<fs::ReadDir>, UpstrokeError> {
    match fs::read_dir(root) {
        Ok(entries) => Ok(Some(entries)),
        Err(source) => {
            if source.kind() == io::ErrorKind::NotFound {
                match fs::symlink_metadata(root) {
                    Err(absent) if absent.kind() == io::ErrorKind::NotFound => {
                        return Ok(None);
                    }
                    Ok(_) => {}
                    Err(source) => {
                        return Err(UpstrokeError::Filesystem {
                            operation: "inspect directory",
                            path: root.to_path_buf(),
                            source,
                        });
                    }
                }
            }
            Err(UpstrokeError::Filesystem {
                operation: "enumerate directory",
                path: root.to_path_buf(),
                source,
            })
        }
    }
}

/// Consume the complete directory stream before any caller can inspect leases
/// or reclaim a run. The iterator boundary permits a deterministic per-entry
/// I/O failure witness without replacing a filesystem effect.
pub(super) fn collect_run_directories(
    root: &Path,
    entries: impl IntoIterator<Item = io::Result<(OsString, PathBuf)>>,
) -> Result<Vec<OsString>, UpstrokeError> {
    let mut runs = Vec::new();
    for entry in entries {
        let (name, path) = entry.map_err(|source| UpstrokeError::Filesystem {
            operation: "enumerate run directories in",
            path: root.to_path_buf(),
            source,
        })?;
        // Follow directory links as the previous enumeration did, but preserve
        // an unresolved target as an error instead of interpreting it as a file.
        let metadata = fs::metadata(&path).map_err(|source| UpstrokeError::Filesystem {
            operation: "inspect run directory entry",
            path,
            source,
        })?;
        if metadata.is_dir() {
            runs.push(name);
        }
    }
    runs.sort();
    Ok(runs)
}

/// The run ids among [`run_dir_names`]: the names that are valid UTF-8.
///
/// A run id is a ULID, 26 characters of Crockford base32, so every directory
/// this engine created is here and a name that is not valid UTF-8 was not this
/// engine's. It is **skipped rather than mangled** — the earlier
/// `to_string_lossy()` enumerated `x` + `0xff` as its neighbour `x` + `U+FFFD`,
/// so the census inspected a directory that did not exist while the real one
/// was never inspected. The filter is UTF-8 validity and nothing more: `x` +
/// `U+FFFD` *is* returned, and it is not a run id either; callers filter as
/// they always did, `list_runs` by commitment and `resolve_run_id` by prefix.
///
/// # Errors
///
/// [`run_dir_names`]'s contextual error, preserved for every reader and census.
pub fn run_ids(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {
    Ok(run_dir_names(repo_root)?
        .into_iter()
        .filter_map(|name| name.to_str().map(str::to_owned))
        .collect())
}

/// Every husk under `<repo>/.upstroke/runs`, oldest first.
///
/// # Errors
///
/// Returns an incomplete enumeration's operation, path, and cause.
pub fn list_husks(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {
    Ok(run_ids(repo_root)?
        .into_iter()
        .filter(|run_id| classify_run_dir(&public_dir(repo_root, run_id)) == RunDirClass::Husk)
        .collect())
}

/// What `status` says about a husk id it was asked for by name.
///
/// `startup_census`: "status is read-only: it ignores husks and, asked
/// explicitly for a husk id, reports an unstarted husk that the next write
/// command reclaims, a retained husk with its reason and locator, or a possibly
/// committed run whose public log has no valid committed first line".
#[derive(Debug)]
pub struct HuskReport {
    pub run_id: String,
    pub public: PathBuf,
    /// The private locator the marker records, when a marker parses.
    pub locator: Option<PathBuf>,
    pub disposition: HuskDisposition,
}

/// What the next write command's census would do with a husk it may reclaim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reclaimable {
    /// Nothing private is bound, so the public half alone is reclaimed.
    PublicOnly(UnboundShape),
    /// The ownership proof holds and no commit record exists: the private half
    /// is reclaimed through the proof-token funnel, then the public directory
    /// with the marker last.
    BothHalves,
}

/// The trichotomy `status` reports a husk id by.
#[derive(Debug)]
pub enum HuskDisposition {
    /// Nothing has started here: the next write command reclaims it.
    Unstarted(Reclaimable),
    /// Retained and reported until the deferred prune command removes it.
    /// [`RetainReason::PossiblyCommitted`] is the third of the three sentences.
    Retained(RetainReason),
}

impl HuskDisposition {
    /// The operator-facing sentence, which names which of the three this is.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Unstarted(Reclaimable::BothHalves) => "an unstarted husk, bound to a private \
                 half that never committed, that the next write command reclaims"
                .to_owned(),
            Self::Unstarted(Reclaimable::PublicOnly(shape)) => format!(
                "an unstarted husk ({}) that the next write command reclaims",
                match shape {
                    UnboundShape::Bare => "a bare directory",
                    UnboundShape::StagedMarkerOnly => "only a staged marker",
                    UnboundShape::TargetAbsent => "its recorded private half is gone",
                }
            ),
            Self::Retained(RetainReason::PossiblyCommitted) => {
                "a possibly committed run whose public log has no valid committed first line; \
                 nothing is deleted"
                    .to_owned()
            }
            Self::Retained(reason) => format!("a retained husk: {reason}"),
        }
    }
}

/// Report a husk by id, for `status` and for the census report.
///
/// Read-only from end to end. The authorized private root is the one the
/// command is configured with, which for a read-only `status` is the default.
#[must_use]
pub fn husk_report(
    repo_root: &Path,
    run_id: &str,
    repo_key: &RepoKey,
    authorized_root: &Path,
) -> HuskReport {
    let public = public_dir(repo_root, run_id);
    let locator = fs::read_to_string(public.join(MARKER))
        .ok()
        .and_then(|text| serde_json::from_str::<CreatingMarker>(&text).ok())
        .map(|marker| PathBuf::from(marker.private_dir));
    let disposition = match prove_private_half_ownership(&public, repo_key, authorized_root) {
        // A token means the husk is provably this run's and never committed —
        // reclaimable, both halves, by the next write command. The token is
        // dropped unspent: `status` is read-only.
        PrivateHalfOwnership::Proven(_) => HuskDisposition::Unstarted(Reclaimable::BothHalves),
        PrivateHalfOwnership::NothingBound(shape) => {
            HuskDisposition::Unstarted(Reclaimable::PublicOnly(shape))
        }
        PrivateHalfOwnership::Retained(reason) => HuskDisposition::Retained(reason),
    };
    HuskReport {
        run_id: run_id.to_owned(),
        public,
        locator,
        disposition,
    }
}

/// The most recent run — what `upstroke status` reports when given no id.
///
/// # Errors
///
/// Returns an incomplete enumeration's operation, path, and cause. Only a
/// completed enumeration without a committed run answers `Ok(None)`.
pub fn latest_run(repo_root: &Path) -> Result<Option<String>, UpstrokeError> {
    Ok(list_runs(repo_root)?.pop())
}

/// Resolve a run id from any unambiguous prefix, so an operator can type the
/// first few characters of a 26-character ULID.
///
/// An exact match wins outright rather than being treated as one candidate
/// among several: a full id is never ambiguous, even if some other run happens
/// to extend it.
///
/// # Errors
///
/// An incomplete enumeration preserves its operation, path, and cause. Missing
/// or ambiguous prefixes return a refusal explaining the available matches.
pub fn resolve_run_id(repo_root: &Path, wanted: &str) -> Result<String, UpstrokeError> {
    let runs = list_runs(repo_root)?;
    let wanted_upper = wanted.to_ascii_uppercase();
    // The entry as it exists on disk, not the uppercased input. The comparison
    // is case-insensitive because a run directory can arrive from a
    // case-insensitive filesystem, and on a case-sensitive one only the real
    // name builds a path that opens — everything downstream joins this id.
    if let Some(matched) = runs.iter().find(|id| id.eq_ignore_ascii_case(wanted)) {
        return Ok(matched.clone());
    }
    let matches: Vec<&String> = runs
        .iter()
        .filter(|id| id.to_ascii_uppercase().starts_with(&wanted_upper))
        .collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(UpstrokeError::Refused {
            message: match husk_matching(repo_root, wanted)? {
                // A directory is there, and it holds no committed `run_started`.
                // Saying "no run matches that id" of a directory the operator
                // can see is the answer that sends them looking for a bug.
                Some(husk) => format!(
                    "`{husk}` never recorded a committed run_started, so there is no run to open \
                     there — ask `upstroke status {husk}` for what it is and what happens to it"
                ),
                None if runs.is_empty() => {
                    format!("no runs found under {}", runs_root(repo_root).display())
                }
                None => format!("no run matches that id; known runs: {}", runs.join(", ")),
            },
        }),
        several => Err(UpstrokeError::Refused {
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

/// The husk a wanted id names, exactly or by unambiguous prefix.
///
/// Only used to explain a refusal, so an ambiguous prefix answers `None`: the
/// operator is told to use more characters by the branch above, not sent to a
/// husk that merely happens to be one of the matches.
fn husk_matching(repo_root: &Path, wanted: &str) -> Result<Option<String>, UpstrokeError> {
    let husks = list_husks(repo_root)?;
    let wanted_upper = wanted.to_ascii_uppercase();
    if let Some(exact) = husks.iter().find(|id| id.eq_ignore_ascii_case(wanted)) {
        return Ok(Some(exact.clone()));
    }
    let mut prefixed = husks
        .iter()
        .filter(|id| id.to_ascii_uppercase().starts_with(&wanted_upper));
    let matched = match (prefixed.next(), prefixed.next()) {
        (Some(first), None) => Some(first.clone()),
        _ => None,
    };
    Ok(matched)
}

/// A question id resolved to the run that raised it.
#[derive(Debug)]
pub struct FoundQuestion {
    pub run_id: String,
    /// The run's public directory — everything `upstroke answer` touches.
    pub public: PathBuf,
    /// The full question id, expanded from whatever prefix was typed.
    pub question_id: String,
}

/// Find the run holding a question, by full id or unambiguous prefix.
///
/// Scans every run rather than requiring the operator to remember which one
/// asked: the notifier hands them a question id, not a run id, so a question
/// id is what the command has to accept.
///
/// # Errors
///
/// Run or question enumeration errors preserve their operation, path, and
/// cause. Missing or ambiguous question identifiers return a refusal. A name
/// that is not UTF-8 cannot be a question identifier and is skipped exactly,
/// without constructing an identifier from its diagnostic rendering.
pub fn find_question(repo_root: &Path, wanted: &str) -> Result<FoundQuestion, UpstrokeError> {
    let wanted_upper = wanted.to_ascii_uppercase();
    let mut exact: Option<FoundQuestion> = None;
    let mut matches: Vec<FoundQuestion> = Vec::new();
    for run_id in list_runs(repo_root)? {
        let public = public_dir(repo_root, &run_id);
        let questions = public.join("questions");
        let Some(entries) = read_directory_if_present(&questions)? else {
            continue;
        };
        for entry in entries {
            let entry = entry.map_err(|source| UpstrokeError::Filesystem {
                operation: "enumerate questions in",
                path: questions.clone(),
                source,
            })?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Some(question_id) = name.strip_suffix(".json") else {
                continue;
            };
            if question_id.eq_ignore_ascii_case(wanted) {
                // Each stored candidate owns its run identity and public path
                // independently of this directory iterator.
                exact = Some(FoundQuestion {
                    run_id: run_id.clone(),
                    public: public.clone(),
                    question_id: question_id.to_owned(),
                });
            } else if question_id.to_ascii_uppercase().starts_with(&wanted_upper) {
                matches.push(FoundQuestion {
                    run_id: run_id.clone(),
                    public: public.clone(),
                    question_id: question_id.to_owned(),
                });
            }
        }
    }
    if let Some(found) = exact {
        return Ok(found);
    }
    match matches.len() {
        1 => matches.pop().ok_or_else(|| UpstrokeError::Refused {
            message: "question vanished while resolving it".to_owned(),
        }),
        0 => Err(UpstrokeError::Refused {
            message: format!(
                "no question with that id under {}",
                runs_root(repo_root).display()
            ),
        }),
        several => Err(UpstrokeError::Refused {
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
