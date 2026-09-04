//! The read-only residue classifier: what a killed Git command left behind, and
//! whether it is the after-phase publication or the interrupted prefix.
//!
//! `decisions.effect_site_inventory.command_internal_sub_effects` writes the
//! predicate as `classify_object_residue(site, worktree)` and defines
//! `ObjectResidue::Internal` by it. The order this module implements is that
//! sentence's: the after-phase reference decides `After` first, and only its
//! absence lets residue decide `Internal`.
//!
//! **Read-only throughout.** Nothing here writes an object, moves a ref, or
//! touches an index -- a classifier that had to *compute* a content-addressed
//! name would be performing the very effect it is classifying, which is why
//! [`ResidueTarget::published`] carries the parent's record instead. The Git
//! inspections it reads through (`git fsck --unreachable`, `rev-parse --verify
//! --quiet`, `worktree list`, `status`, `diff --cached`) are the parent's
//! helpers, and so is every process start.
//!
//! **§6 and §7.** No shared ownership, lock or clone: [`ResidueTarget`] is
//! four borrows and is `Copy`. Every `?` in this module propagates the
//! parent's `UpstrokeError` unchanged, and each is deliberate in §7's sense:
//! the helper that failed already names what a reader of the failure needs
//! (the git command and the worktree it ran in, or the path an I/O error was
//! about); the crate's error type has no variant that wraps one error in
//! another, so a `map_err` here would flatten an `Io` into a `Git` message and
//! lose its kind; and the classifier's callers (`candidate::verify_object`,
//! `WorkspaceManager::quiescence`, the sampling harnesses) propagate or record
//! the error rather than match on it. What the module decides itself is the
//! one thing §7 puts on it: **an inspection that fails is an error, never an
//! answer.** Every residue name is read through [`name_present`], which makes
//! only an actual not-found "absent"; a permission failure, a symlink loop or
//! a transient I/O error is an `Io` error naming the name. Before the sweep
//! every name went through `Path::exists`, which answers `false` for all of
//! those, so a git dir this process could not search classified as `None`.
//!
//! **What this module cannot promise, and where the promise belongs.** The
//! line above holds for the names this module reads itself. It does not hold
//! for what it reads *through*: the parent's inspections still fold some Git
//! failures into an answer before the `?` here ever sees them — a failed
//! `worktree list` or `cat-file`, a `show-ref` that could not run, a `fsck`
//! that did not finish. Making those trustworthy means reading a repository
//! the way Git reads it (its gitfile grammar, its linked-worktree reader, its
//! trace-polluted streams, with a bound on every read of a
//! repository-controlled file), which is the parent's work and not a child
//! classifier's: `reviews/FINDINGS.md` §51 carries a row per case for the
//! sweep of `src/workspace_manager.rs`, the queue's last row of this family.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/workspace_manager.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// unless it says otherwise -- `PR6-LANEF-004`, and the mistake two W1 pull
// requests then made independently (#100 and #102). Nothing here reaches a
// governed primitive, so all three are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::fs;
use std::path::Path;

use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, ObjectResidue, ObjectSite, ResidueElement, SnapshotSite, WorktreeSite,
};

use super::{
    git_dir_of, head_commit, index_differs_from_head, object_exists, record_for,
    temporary_object_files, unreachable_objects, worktree_has_unstaged_changes,
};

/// What the parent recorded of a site's after-phase publication.
///
/// **The packet writes the predicate as `classify_object_residue(site,
/// worktree)`** (`decisions.effect_site_inventory.command_internal_sub_effects`),
/// and for five of the nine sites that is all it needs. For the other four it
/// is not implementable, and the reason is a property of Git rather than of
/// this module: `write-tree`, the two commit-tree sites, and the proposal
/// cherry-pick publish a **content-addressed** object, so "the command
/// completed" and "the command never ran" leave object stores that differ only
/// in an object whose name the classifier would have to compute — and computing
/// it is the effect. So the second argument carries the worktree *and* what the
/// parent recorded, which is exactly the datum `IdUnread` is defined by the
/// absence of.
///
/// [`Self::new`] is the five-site form; [`Self::published`] adds the record.
// `Copy` (§6): four borrows and nothing owned, so a copy is the value's
// intended semantics and no caller has to clone it.
#[derive(Debug, Clone, Copy)]
pub struct ResidueTarget<'a> {
    repository: &'a Path,
    worktree: &'a Path,
    published: Option<&'a str>,
    base: Option<&'a str>,
}

impl<'a> ResidueTarget<'a> {
    /// The worktree the site's Git command ran in — for the two commit-tree
    /// sites, the repository the object was written into.
    #[must_use]
    pub fn new(repository: &'a Path) -> Self {
        Self {
            repository,
            worktree: repository,
            published: None,
            base: None,
        }
    }

    /// The site's owning worktree, when it is not the repository itself.
    ///
    /// Given separately because the worktree of a killed `worktree add` may not
    /// exist at all, and a classifier that asked *it* which worktrees are
    /// registered would answer "none registered" for the very residue it is
    /// there to recognise.
    #[must_use]
    pub fn at(mut self, worktree: &'a Path) -> Self {
        self.worktree = worktree;
        self
    }

    /// The object id the parent read and recorded, for the sites whose
    /// after-phase reference is a **content-addressed object** it must name to
    /// tell "written" from "never written".
    #[must_use]
    pub fn published(mut self, object: &'a str) -> Self {
        self.published = Some(object);
        self
    }

    /// The commit the site's worktree was checked out at, for the site whose
    /// after-phase reference is *movement* of that worktree's HEAD.
    ///
    /// `Object.ProposalCherryPick` is the one: `resource_accounting[R10]` says
    /// "its detached HEAD and index reference the proposal commit … while it
    /// exists", so the after phase is a fact about the staging HEAD rather than
    /// about anything the parent recorded — and the base it moved off is known
    /// before the command runs, because `Worktree.AddStaging` checked it out.
    /// A kill therefore cannot lose it, which is why this site does not need
    /// the parent's record the way the object-printing sites do.
    #[must_use]
    pub fn from_base(mut self, base: &'a str) -> Self {
        self.base = Some(base);
        self
    }

    /// The repository the objects live in.
    #[must_use]
    pub fn repository(&self) -> &Path {
        self.repository
    }

    /// The worktree.
    #[must_use]
    pub fn worktree(&self) -> &Path {
        self.worktree
    }
}

/// Every site the classifier is total over, derived from the frozen enums.
///
/// `command_internal_sub_effects`: "the classifier is total over `{None,
/// Internal, After}` for **every Object site** and for `Worktree.Add` /
/// `Snapshot.Add`". The list is not written out here: it is every site whose
/// `residue_classes()` is non-empty, which is what PR3 froze and what
/// `ObjectSite::residue_classes` and `WorktreeSite::residue_classes` answer.
/// Enumerating it by hand is the `bounded_grid` failure this project has
/// recorded three times — a grid over the sites its author remembered.
#[must_use]
pub fn residue_classified_sites() -> Vec<EffectSiteId> {
    EffectSiteId::all()
        .into_iter()
        .filter(|site| !site.residue_classes().is_empty())
        .collect()
}

/// The read-only inspection predicate of
/// `decisions.effect_site_inventory.command_internal_sub_effects`.
///
/// > "the prefix objects-written-reference-unpublished is registered as the
/// > residue class `ObjectResidue::Internal`, defined by the read-only
/// > inspection predicate `classify_object_residue(site, worktree)`: unreachable
/// > objects per `git fsck --unreachable` and/or Git temporary object files
/// > (R27; Git prunes both) plus administrative residue in the owning
/// > worktree's git dir … or a registered-but-unpopulated worktree, **with the
/// > after-phase reference absent**".
///
/// The order is that sentence's: the after-phase reference decides `After`
/// first, and only its absence lets residue decide `Internal`.
///
/// Read-only. Nothing here writes an object, moves a ref, or touches an index.
///
/// # Errors
///
/// [`UpstrokeError::Git`] from one of the parent's read-only inspections;
/// [`UpstrokeError::Io`] naming a path this module could not inspect — a
/// residue name in a git dir, or the worktree's `.git` pointer (§7: only an
/// actual not-found is absence, and an inspection this module makes and
/// cannot complete is never an answer); or [`UpstrokeError::Refused`] for a
/// site the frozen enums register no residue class for — the classifier is
/// total over its domain and silent outside it, rather than answering `None`
/// for a question nobody asked.
pub fn classify_object_residue(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<ObjectResidue, UpstrokeError> {
    if site.residue_classes().is_empty() {
        return Err(UpstrokeError::Refused {
            message: format!(
                "`{site}` registers no residue class, so classify_object_residue has nothing to \
                 be total over there"
            ),
        });
    }
    // The three adds are one reading: `add_state` is read once here, and its
    // three states are the three classes, so no transition between an
    // after-phase read and a residue read can yield `None` for a registered
    // worktree. (`observed_residue_elements` reads it again for its own
    // callers; a caller that calls both reads twice, and this function does
    // not.)
    if let EffectSiteId::Worktree(WorktreeSite::Add | WorktreeSite::AddStaging)
    | EffectSiteId::Snapshot(SnapshotSite::Add) = site
    {
        return Ok(match add_state(target.repository, target.worktree)? {
            AddState::Populated => ObjectResidue::After,
            AddState::Unpopulated => ObjectResidue::Internal,
            AddState::Unregistered => ObjectResidue::None,
        });
    }
    if after_reference_present(site, target)? {
        return Ok(ObjectResidue::After);
    }
    if internal_residue_present(site, target)? {
        return Ok(ObjectResidue::Internal);
    }
    Ok(ObjectResidue::None)
}

/// Whether the site's after-phase reference is present, for the Object sites.
///
/// The three adds are classified in [`classify_object_residue`] from one
/// `add_state` reading and never reach here; an add site here is the refusal
/// at the bottom, as any site without an arm is.
fn after_reference_present(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<bool, UpstrokeError> {
    let worktree = target.worktree;
    let repository = target.repository;
    // Every `?` here propagates a parent inspection unchanged: the failure
    // already names its git command and worktree, or its path (module doc,
    // §7).
    match site {
        // `git add -A` publishes its blobs by renaming index.lock over index.
        // A surviving lock is proof the publication did not happen; otherwise
        // the after state is an index that reflects the working tree.
        EffectSiteId::Object(ObjectSite::CandidateStage) => {
            if index_lock_present(worktree)? {
                return Ok(false);
            }
            Ok(!worktree_has_unstaged_changes(worktree)?)
        }
        // `write-tree` publishes its trees through the index's cache-tree
        // extension, which is a fsck root — so the recorded tree being present
        // *and reachable* is the after phase, and an unreachable one is the
        // interrupted prefix.
        EffectSiteId::Object(ObjectSite::CandidateWriteTree) => {
            if index_lock_present(worktree)? {
                return Ok(false);
            }
            let Some(published) = target.published else {
                return Ok(false);
            };
            Ok(object_exists(repository, published)?
                && !unreachable_objects(repository)?
                    .iter()
                    .any(|id| id == published))
        }
        // The commit-tree sites: `AfterEffect::Unreferenced`. The object is
        // present and nothing references it — the after phase and the R27
        // residue differ only in whether the parent recorded the id, which is
        // what `IdUnread` is.
        EffectSiteId::Object(ObjectSite::SnapshotCommitTree | ObjectSite::CandidateCommitTree) => {
            let Some(published) = target.published else {
                return Ok(false);
            };
            object_exists(repository, published)
        }
        // The proposal cherry-pick publishes its objects through the staging
        // HEAD.
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => {
            if index_lock_present(worktree)? {
                return Ok(false);
            }
            let Some(head) = head_commit(worktree)? else {
                return Ok(false);
            };
            if let Some(published) = target.published {
                return Ok(head == published);
            }
            Ok(target.base.is_some_and(|base| head != base))
        }
        // `cherry-pick --no-commit` publishes its merge objects through the
        // repair worktree's index. CHERRY_PICK_HEAD survives a *successful*
        // `--no-commit`, so it is never the discriminator here.
        EffectSiteId::Object(ObjectSite::RepairMaterialize) => {
            if index_lock_present(worktree)? {
                return Ok(false);
            }
            index_differs_from_head(worktree)
        }
        other => Err(UpstrokeError::Refused {
            message: format!("`{other}` has no after-phase reference the classifier knows"),
        }),
    }
}

/// Whether the command-internal residue of `site` is present.
fn internal_residue_present(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<bool, UpstrokeError> {
    Ok(!observed_residue_elements(site, target)?.is_empty())
}

/// Which of the site's own registered residue elements are present.
///
/// The element list is [`EffectSiteId::residue_elements`] — PR3's, frozen —
/// rather than a list written here. A classifier that recognised elements its
/// site does not register would answer `Internal` for states the fault matrix
/// never tables, and one that recognised fewer would answer `None` for durable
/// state no action recovers.
///
/// # Errors
///
/// [`UpstrokeError::Git`] from one of the parent's read-only inspections, or
/// [`UpstrokeError::Io`] naming a path this module could not inspect: a
/// residue name in a git dir, or the worktree's `.git` pointer.
pub fn observed_residue_elements(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<Vec<ResidueElement>, UpstrokeError> {
    let worktree = target.worktree;
    let repository = target.repository;
    let mut present = Vec::new();
    // Every `?` here propagates a parent inspection unchanged (module doc,
    // §7); the one read this module makes itself is `name_present`.
    let git_dir = git_dir_of(worktree)?;
    // A name in the owning worktree's git dir; no git dir, no name.
    let in_git_dir = |name: &str| match git_dir.as_deref() {
        Some(dir) => name_present(dir, name),
        None => Ok(false),
    };
    for element in site.residue_elements() {
        let seen = match element {
            ResidueElement::UnreferencedObject => {
                let unreachable = unreachable_objects(repository)?;
                match target.published {
                    Some(published) => unreachable.iter().any(|id| id != published),
                    None => !unreachable.is_empty(),
                }
            }
            ResidueElement::TemporaryObjectFile => temporary_object_files(repository)?,
            ResidueElement::IndexLock => in_git_dir("index.lock")?,
            ResidueElement::CherryPickHead => in_git_dir("CHERRY_PICK_HEAD")?,
            ResidueElement::MergeHead => in_git_dir("MERGE_HEAD")?,
            ResidueElement::MergeMsg => in_git_dir("MERGE_MSG")?,
            ResidueElement::OrigHead => in_git_dir("ORIG_HEAD")?,
            ResidueElement::SequencerState => in_git_dir("sequencer")?,
            ResidueElement::RegisteredUnpopulatedWorktree => {
                add_state(repository, worktree)? == AddState::Unpopulated
            }
        };
        if seen {
            present.push(*element);
        }
    }
    Ok(present)
}

/// Whether an element makes the worktree it sits in non-quiescent.
///
/// **A counted, stated boundary.** `command_internal_sub_effects` says of the
/// synthetic evidence that for each element "`classify_object_residue` returns
/// `Internal`, **`Worktree.Verify` fails**, and the tabled recovery converges".
/// That is true of every element that lives in the owning worktree's git dir
/// and of a registered-but-unpopulated worktree. It is *not* true of
/// [`ResidueElement::UnreferencedObject`] or
/// [`ResidueElement::TemporaryObjectFile`]: those live in the shared object
/// store, are R27 — "Git's" — and are left by ordinary Git use (every amended
/// commit leaves one). A `Worktree.Verify` that consulted the object store
/// would refuse to reuse an `OpenNoAttempt` worktree in essentially every real
/// repository, which `decisions.workspace_candidates.generation` requires it to
/// reuse.
///
/// So the suite asserts the `Verify`-fails half for the elements it holds of,
/// asserts its *negation* for the other two, and asserts the partition as a
/// count — see `every_registered_residue_element_is_constructed_and_recovers`.
#[must_use]
pub const fn element_breaks_quiescence(element: ResidueElement) -> bool {
    match element {
        ResidueElement::UnreferencedObject | ResidueElement::TemporaryObjectFile => false,
        ResidueElement::IndexLock
        | ResidueElement::CherryPickHead
        | ResidueElement::MergeHead
        | ResidueElement::MergeMsg
        | ResidueElement::OrigHead
        | ResidueElement::SequencerState
        | ResidueElement::RegisteredUnpopulatedWorktree => true,
    }
}

/// The administrative residue in one worktree's git dir, in the order
/// `command_internal_sub_effects` lists it.
///
/// `ORIG_HEAD` is deliberately absent from what makes a worktree non-quiescent
/// here even though the sentence lists it: no site's frozen
/// `residue_elements()` registers it, and `git reset`, `git merge` and
/// `git rebase` all write one in the ordinary course of events, so reading it
/// as evidence of an interrupted command would close generations that are
/// perfectly reusable. Recorded rather than silently dropped.
///
/// # Errors
///
/// An I/O error naming a name in the git dir this process could not inspect.
pub(super) fn administrative_residue_at(
    git_dir: &Path,
) -> Result<Vec<ResidueElement>, UpstrokeError> {
    let mut present = Vec::new();
    for (name, element) in [
        ("index.lock", ResidueElement::IndexLock),
        ("CHERRY_PICK_HEAD", ResidueElement::CherryPickHead),
        ("MERGE_HEAD", ResidueElement::MergeHead),
        ("MERGE_MSG", ResidueElement::MergeMsg),
        ("sequencer", ResidueElement::SequencerState),
        ("rebase-merge", ResidueElement::SequencerState),
        ("rebase-apply", ResidueElement::SequencerState),
        ("REVERT_HEAD", ResidueElement::SequencerState),
    ] {
        if name_present(git_dir, name)? {
            present.push(element);
        }
    }
    Ok(present)
}

/// What one of the three adds left behind: their three residue classes.
///
/// The after phase is `Populated` and the site's one residue element,
/// `RegisteredUnpopulatedWorktree`, is `Unpopulated`: [`classify_object_residue`]
/// reads this once and maps the three states to the three classes, so no
/// state can be both, or neither while registered, and no transition between
/// two reads can be read as `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddState {
    /// `git worktree list` does not name the worktree.
    Unregistered,
    /// Registered, and the population did not finish: `git worktree add`
    /// holds an `initializing` lock for the whole of its run, so a surviving
    /// lock is Git's own statement to that effect; or no git dir is behind
    /// the worktree's `.git` as `git_dir_of` reads it, which is where a
    /// pointer file a kill left empty, or one that is not a pointer, lands,
    /// whatever `Path::exists` says of the name.
    Unpopulated,
    /// Registered, unlocked, and a git dir is behind the pointer.
    Populated,
}

///
/// What it reads is the parent's `record_for` and `git_dir_of`, so it is only
/// as trustworthy as those are: `record_for` answers `None` for a `worktree
/// list` that failed, and `git_dir_of` accepts any target text after
/// `gitdir:`. Both are rows for the parent's sweep in `reviews/FINDINGS.md`
/// §51; this function's own contribution is that the after phase and the
/// residue element are two arms of one reading rather than two hand-written
/// complements.
fn add_state(repository: &Path, worktree: &Path) -> Result<AddState, UpstrokeError> {
    let Some(record) = record_for(repository, worktree)? else {
        return Ok(AddState::Unregistered);
    };
    if record.locked.as_deref() == Some("initializing") || git_dir_of(worktree)?.is_none() {
        return Ok(AddState::Unpopulated);
    }
    Ok(AddState::Populated)
}

/// Whether the worktree's index lock survives.
///
/// `git add`, `write-tree` and the cherry-picks all publish through
/// `index.lock` renamed over `index`, so a surviving lock is the interrupted
/// prefix. No git dir behind the worktree's `.git`, no lock.
fn index_lock_present(worktree: &Path) -> Result<bool, UpstrokeError> {
    match git_dir_of(worktree)? {
        Some(dir) => name_present(&dir, "index.lock"),
        None => Ok(false),
    }
}

/// Whether `name` is present in `dir`, from the name's own metadata.
///
/// §7: only an actual not-found is absence. `Path::exists` answers `false` for
/// a permission failure, a symlink loop and a transient I/O error as well, and
/// every question this module asks of a name is a residue question, so a name
/// it could not inspect is an `Io` error naming the name, never "no residue".
/// The read does not follow a symlink: these are names Git takes with `O_EXCL`
/// and releases by unlinking, so the name is the fact whatever it points at —
/// measured on git 2.43, an `index.lock` that is a dangling symlink makes
/// `git add` fail with "File exists".
fn name_present(dir: &Path, name: &str) -> Result<bool, UpstrokeError> {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(UpstrokeError::Io { path, source }),
    }
}
