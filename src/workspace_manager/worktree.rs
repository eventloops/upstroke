//! What Git reports about a linked worktree, and what
//! [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
//! demands of one before it is reused.
//!
//! `decisions.workspace_candidates.generation`: "a worktree is reused across a
//! process boundary or after an interrupted Git command … only after
//! Worktree.Verify". These are the three values that conversation is held in --
//! the record Git hands back, the quiescence the caller asks for, and the
//! reasons the answer can be no. The verification itself runs a Git child and
//! is the parent's.

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

use std::fmt;
use std::path::PathBuf;

use crate::topology::effects::ResidueElement;

/// A registered worktree of the managed repository, as
/// `git worktree list --porcelain -z` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    /// The checkout path, decoded byte-safely.
    pub path: PathBuf,
    /// The commit its HEAD names, when it has one.
    pub head: Option<String>,
    /// The branch it has checked out, when it is not detached.
    pub branch: Option<String>,
    /// Git's own lock reason. `git worktree add` holds `initializing` for the
    /// whole of its run and releases it only once the checkout is populated, so
    /// this field is how a registered-but-unpopulated worktree announces
    /// itself.
    pub locked: Option<String>,
    /// Git's own prunable reason.
    pub prunable: Option<String>,
}

/// Why [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
/// refused to reuse a worktree.
///
/// `decisions.workspace_candidates.generation`: "a worktree is reused across a
/// process boundary or after an interrupted Git command … only after
/// Worktree.Verify: the recorded path is a linked worktree of this repository,
/// HEAD equals the recorded base (or, for RetainedIdle, the worktree holds the
/// retained cumulative tree), the index is unlocked, and no
/// cherry-pick/merge/revert/sequencer/rebase state exists".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyFailure {
    /// Nothing is registered at the recorded path.
    NotRegistered,
    /// Registered, and `git worktree add` never finished populating it — the
    /// `registered-but-unpopulated` residue element.
    Unpopulated,
    /// Registered at the path but belonging to a different repository.
    ForeignRepository,
    /// The checkout directory is gone.
    Missing,
    /// HEAD is not the recorded base.
    HeadMismatch {
        /// The recorded base.
        expected: String,
        /// What HEAD actually is.
        actual: String,
    },
    /// The retained cumulative tree is not the one the worktree holds.
    TreeMismatch {
        /// The recorded tree.
        expected: String,
        /// Why the index does not hold it: the paths that differ, or the reason
        /// the comparison could not be made against that tree at all.
        ///
        /// This was the tree the index writes out as, and obtaining it meant
        /// running `git write-tree`, which **writes** (`PR5-CONF-002`). A
        /// read-only observation cannot name a tree object that does not exist
        /// yet, so it names the difference instead — which is the more useful
        /// half of that diagnostic anyway.
        difference: String,
    },
    /// Administrative residue of an interrupted command.
    Residue(ResidueElement),
}

impl fmt::Display for VerifyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegistered => f.write_str("no worktree is registered at the recorded path"),
            Self::Unpopulated => f.write_str(
                "the worktree is registered and was never populated: `git worktree add` still \
                 holds its `initializing` lock",
            ),
            Self::ForeignRepository => {
                f.write_str("the worktree at the recorded path belongs to another repository")
            }
            Self::Missing => f.write_str("the worktree's checkout directory is gone"),
            Self::HeadMismatch { expected, actual } => {
                write!(f, "HEAD is {actual}, not the recorded base {expected}")
            }
            Self::TreeMismatch {
                expected,
                difference,
            } => write!(
                f,
                "the worktree does not hold the retained cumulative tree {expected}: {difference}"
            ),
            Self::Residue(element) => write!(
                f,
                "administrative residue of an interrupted command is present: {element:?}"
            ),
        }
    }
}

/// What a worktree has to hold for
/// [`WorkspaceManager::verify_worktree`](super::WorkspaceManager::verify_worktree)
/// to pass it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quiescence {
    /// The ordinary case: HEAD equals the recorded base.
    AtBase(String),
    /// `RetainedIdle`: "the worktree holds the retained cumulative tree".
    HoldsTree(String),
}
