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
//! inspections it reads through (`git fsck --unreachable`, `cat-file -e`,
//! `worktree list`, `status`, `diff --cached`) are the parent's helpers, and so
//! is every process start.

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

use std::path::Path;

use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, ObjectResidue, ObjectSite, ResidueElement, SnapshotSite, WorktreeSite,
};

use super::{
    git_dir_of, head_commit, index_differs_from_head, index_lock_present, object_exists,
    record_for, temporary_object_files, unreachable_objects, worktree_has_unstaged_changes,
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
#[derive(Debug, Clone)]
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
/// A Git or I/O error, or [`UpstrokeError::Refused`] for a site the frozen enums
/// register no residue class for — the classifier is total over its domain and
/// silent outside it, rather than answering `None` for a question nobody asked.
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
    if after_reference_present(site, target)? {
        return Ok(ObjectResidue::After);
    }
    if internal_residue_present(site, target)? {
        return Ok(ObjectResidue::Internal);
    }
    Ok(ObjectResidue::None)
}

/// Whether the site's after-phase reference is present.
fn after_reference_present(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<bool, UpstrokeError> {
    let worktree = target.worktree;
    let repository = target.repository;
    match site {
        // The three adds: registered *and* populated. `git worktree add` holds
        // an `initializing` lock for the whole of its run, so a surviving lock
        // is Git's own statement that the population did not finish.
        EffectSiteId::Worktree(WorktreeSite::Add | WorktreeSite::AddStaging)
        | EffectSiteId::Snapshot(SnapshotSite::Add) => {
            let Some(record) = record_for(repository, worktree)? else {
                return Ok(false);
            };
            Ok(record.locked.as_deref() != Some("initializing") && worktree.join(".git").exists())
        }
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
/// A Git or I/O error.
pub fn observed_residue_elements(
    site: EffectSiteId,
    target: &ResidueTarget<'_>,
) -> Result<Vec<ResidueElement>, UpstrokeError> {
    let worktree = target.worktree;
    let repository = target.repository;
    let mut present = Vec::new();
    let git_dir = git_dir_of(worktree)?;
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
            ResidueElement::IndexLock => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("index.lock").exists()),
            ResidueElement::CherryPickHead => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("CHERRY_PICK_HEAD").exists()),
            ResidueElement::MergeHead => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("MERGE_HEAD").exists()),
            ResidueElement::MergeMsg => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("MERGE_MSG").exists()),
            ResidueElement::OrigHead => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("ORIG_HEAD").exists()),
            ResidueElement::SequencerState => git_dir
                .as_ref()
                .is_some_and(|dir| dir.join("sequencer").exists()),
            ResidueElement::RegisteredUnpopulatedWorktree => record_for(repository, worktree)?
                .is_some_and(|record| {
                    record.locked.as_deref() == Some("initializing")
                        || !worktree.join(".git").exists()
                }),
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
        if git_dir.join(name).exists() {
            present.push(element);
        }
    }
    Ok(present)
}
