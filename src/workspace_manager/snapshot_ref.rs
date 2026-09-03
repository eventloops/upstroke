//! What an exact snapshot is checked out at, and what one is once it exists.
//!
//! `decisions.workspace_candidates.snapshots` distinguishes the two inputs a
//! snapshot can be taken of -- an existing commit, for which no object is
//! created, and a tree, for which the funnel first writes an ephemeral commit
//! on the recorded parent. [`SnapshotInput`] is that distinction as a value and
//! [`Snapshot`] is what the funnel returns; the funnel, the ephemeral
//! commit-tree it may run, and the ref discipline around it are the parent's.

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

use std::path::PathBuf;

use super::Slot;

/// What a snapshot is checked out at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotInput {
    /// The integration case: an existing commit, and no object is created.
    Commit(String),
    /// The candidate case: a tree, for which the funnel first writes an
    /// ephemeral commit on `parent`.
    Tree {
        /// The immutable tree under judgment.
        tree: String,
        /// The recorded parent the ephemeral commit sits on.
        parent: String,
    },
}

/// One live exact snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Its slot.
    pub slot: Slot,
    /// Its checkout.
    pub path: PathBuf,
    /// The commit its detached HEAD names.
    pub head: String,
    /// The ephemeral commit this snapshot created, when its input was a tree.
    /// It returns to R27 when the snapshot is removed.
    pub ephemeral: Option<String>,
}
