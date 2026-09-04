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
/// This is the slice's only change in behaviour: a legacy husk that today
/// shadows [`latest_run`] is no longer listed. A run whose log committed is
/// listed exactly as before, marker or no marker.
pub fn list_runs(repo_root: &Path) -> Vec<String> {
    let mut runs: Vec<String> = run_dir_names(repo_root)
        .into_iter()
        .filter(|run_id| classify_run_dir(&public_dir(repo_root, run_id)) == RunDirClass::Committed)
        .collect();
    runs.sort();
    runs
}

/// Every directory under `<repo>/.upstroke/runs` **whose name the filesystem
/// and this crate spell the same way**, committed or not, oldest first.
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
/// A name that does not round-trip is therefore **skipped rather than
/// mangled**. The filter is exactly that and nothing more: it asks whether the
/// name is valid UTF-8, not whether it is a run id. So this still returns names
/// no run ever had — `x` + `U+FFFD` is returned, and it is not a ULID — and
/// callers filter as they always did (`list_runs` by commitment,
/// `resolve_run_id` by prefix). What it no longer does is return a name that
/// does **not** name the directory it came from, which is the property the
/// census needs: every name here opens the entry it was read from.
///
/// The narrowing is real and it is bounded: a run id is a ULID, 26 characters
/// of Crockford base32, so no directory this engine creates is ever skipped.
/// One that is skipped was not this engine's, and skipping it is strictly
/// better than inspecting a phantom in its place. The alternative — carrying
/// `OsString` out of here — reaches `list_runs`, `resolve_run_id`, `status` and
/// the event log's own run ids, which is a different change in a file whose
/// sweep has not run (queue row 13).
#[must_use]
pub fn run_dir_names(repo_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(runs_root(repo_root)) else {
        return Vec::new();
    };
    let mut runs: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    runs.sort();
    runs
}

/// Every husk under `<repo>/.upstroke/runs`, oldest first.
#[must_use]
pub fn list_husks(repo_root: &Path) -> Vec<String> {
    run_dir_names(repo_root)
        .into_iter()
        .filter(|run_id| classify_run_dir(&public_dir(repo_root, run_id)) == RunDirClass::Husk)
        .collect()
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
pub fn latest_run(repo_root: &Path) -> Option<String> {
    list_runs(repo_root).pop()
}

/// Resolve a run id from any unambiguous prefix, so an operator can type the
/// first few characters of a 26-character ULID.
///
/// An exact match wins outright rather than being treated as one candidate
/// among several: a full id is never ambiguous, even if some other run happens
/// to extend it.
pub fn resolve_run_id(repo_root: &Path, wanted: &str) -> Result<String, UpstrokeError> {
    let runs = list_runs(repo_root);
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
            message: match husk_matching(repo_root, wanted) {
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
fn husk_matching(repo_root: &Path, wanted: &str) -> Option<String> {
    let husks = list_husks(repo_root);
    let wanted_upper = wanted.to_ascii_uppercase();
    if let Some(exact) = husks.iter().find(|id| id.eq_ignore_ascii_case(wanted)) {
        return Some(exact.clone());
    }
    let mut prefixed = husks
        .iter()
        .filter(|id| id.to_ascii_uppercase().starts_with(&wanted_upper));
    let first = prefixed.next()?;
    prefixed.next().is_none().then(|| first.clone())
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
pub fn find_question(repo_root: &Path, wanted: &str) -> Result<FoundQuestion, UpstrokeError> {
    let wanted_upper = wanted.to_ascii_uppercase();
    let mut exact: Option<FoundQuestion> = None;
    let mut matches: Vec<FoundQuestion> = Vec::new();
    for run_id in list_runs(repo_root) {
        let public = public_dir(repo_root, &run_id);
        let Ok(entries) = fs::read_dir(public.join("questions")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(question_id) = name.strip_suffix(".json") else {
                continue;
            };
            let found = FoundQuestion {
                run_id: run_id.clone(),
                public: public.clone(),
                question_id: question_id.to_owned(),
            };
            if question_id.eq_ignore_ascii_case(wanted) {
                exact = Some(found);
            } else if question_id.to_ascii_uppercase().starts_with(&wanted_upper) {
                matches.push(found);
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
