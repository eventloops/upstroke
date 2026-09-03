//! The slot names, the snapshot names, and the intent record they are written
//! into.
//!
//! `decisions.workspace_candidates.manager` names two of the three namespaces
//! literally -- "detached linked worktrees with durable synced intents
//! (`tasks/k<key>-g<gen>`, `merge/s<seq>`)" -- and
//! `decisions.workspace_candidates.snapshots` requires the third's members to be
//! "never reused across roles or attempts". Both are properties of the *name*
//! here rather than of the caller's discipline: [`SnapshotName`] can only be
//! built by one of its three constructors, and [`safe_component`] is what makes
//! a [`Slot`] path containment-by-construction.
//!
//! Pure string and path arithmetic. Nothing in this module reads or writes the
//! filesystem; the funnels that act on the paths it names are the parent's.
//!
//! **[`Slot`]'s five effect-site accessors are deliberately not here.** `row`,
//! `add_site`, `write_intent_site`, `remove_site` and `remove_intent_site` map a
//! slot to the [`EffectSiteId`](crate::topology::effects::EffectSiteId) its
//! funnel runs under, which is effect-site vocabulary rather than naming, and
//! `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_
//! it_or_is_recorded_absent` reads `src/workspace_manager.rs` **by path** for
//! exactly those eleven variant literals. They stay in the parent, in a second
//! `impl Slot` block beside the module declaration, so that census keeps
//! measuring what it measured before the split.

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

use serde::{Deserialize, Serialize};

use super::Refusal;

/// The three worktree namespaces of an execution root.
///
/// `decisions.workspace_candidates.manager` names two of them literally —
/// "detached linked worktrees with durable synced intents (`tasks/k<key>-g<gen>`,
/// `merge/s<seq>`)" — and `snapshots` names the third, whose members
/// `decisions.workspace_candidates.snapshots` requires to be "never reused
/// across roles or attempts".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// `tasks/k<key>-g<gen>` — a task worktree, R9.
    Task {
        /// The task key.
        key: String,
        /// The generation number.
        generation: u32,
    },
    /// `merge/s<seq>` — a staging worktree, R10. Never created for an
    /// exact-base fast sequence.
    Staging {
        /// The merge sequence number.
        sequence: u64,
    },
    /// `snapshots/<name>` — an exact gate or review snapshot, R24.
    Snapshot {
        /// The snapshot's name, which encodes its role, generation, and
        /// attempt so that no two roles or attempts can collide.
        name: SnapshotName,
    },
}

/// A snapshot's name, built so that "never reused across roles or attempts" is
/// a property of the name rather than of the caller's discipline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotName(String);

impl SnapshotName {
    /// The one snapshot the whole gate set runs on.
    #[must_use]
    pub fn gates(generation: u32, attempt: u32) -> Self {
        Self(format!("g{generation}-a{attempt}-gates"))
    }

    /// One fresh snapshot per reviewer.
    #[must_use]
    pub fn review(generation: u32, attempt: u32, reviewer: u32) -> Self {
        Self(format!("g{generation}-a{attempt}-review{reviewer}"))
    }

    /// The snapshot an integration transaction judges its proposal on.
    #[must_use]
    pub fn integration(sequence: u64) -> Self {
        Self(format!("s{sequence}-integration"))
    }

    /// The name as a directory component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SnapshotName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether `name` is safe as a single path component.
pub(super) fn safe_component(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("it is empty");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Some("only ASCII alphanumerics, `-`, and `_` are legal in a slot component");
    }
    if name.starts_with('-') {
        return Some(
            "a leading `-` would be read as an option by the Git commands the funnels run",
        );
    }
    None
}

impl Slot {
    /// The slot's path relative to the execution root.
    #[must_use]
    pub fn relative(&self) -> PathBuf {
        match self {
            Self::Task { key, generation } => {
                PathBuf::from("tasks").join(format!("k{key}-g{generation}"))
            }
            Self::Staging { sequence } => PathBuf::from("merge").join(format!("s{sequence}")),
            Self::Snapshot { name } => PathBuf::from("snapshots").join(name.as_str()),
        }
    }

    /// The intent file's name, injective over slots: the two components are
    /// joined by `.`, which [`safe_component`] forbids inside either.
    #[must_use]
    pub fn intent_name(&self) -> String {
        match self {
            Self::Task { key, generation } => format!("tasks.k{key}-g{generation}.intent"),
            Self::Staging { sequence } => format!("merge.s{sequence}.intent"),
            Self::Snapshot { name } => format!("snapshots.{name}.intent"),
        }
    }

    /// What the intent record calls this kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Task { .. } => "task",
            Self::Staging { .. } => "staging",
            Self::Snapshot { .. } => "snapshot",
        }
    }

    /// Refuse a slot whose components could escape the execution root.
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let (kind, name) = match self {
            Self::Task { key, .. } => ("task", key.as_str()),
            Self::Staging { .. } => return Ok(()),
            Self::Snapshot { name } => ("snapshot", name.as_str()),
        };
        match safe_component(name) {
            None => Ok(()),
            Some(why) => Err(Refusal::SlotName {
                kind,
                name: name.to_owned(),
                why,
            }),
        }
    }

    /// Rebuild a slot from an intent file name, so reclaim never has to trust
    /// a path stored inside a record.
    pub(super) fn from_intent_name(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".intent")?;
        if let Some(rest) = stem.strip_prefix("tasks.k") {
            let (key, generation) = rest.rsplit_once("-g")?;
            return Some(Self::Task {
                key: key.to_owned(),
                generation: generation.parse().ok()?,
            });
        }
        if let Some(rest) = stem.strip_prefix("merge.s") {
            return Some(Self::Staging {
                sequence: rest.parse().ok()?,
            });
        }
        if let Some(rest) = stem.strip_prefix("snapshots.") {
            return Some(Self::Snapshot {
                name: SnapshotName(rest.to_owned()),
            });
        }
        None
    }
}

/// The durable per-owner recovery record `resource_accounting` requires of
/// every worktree, staging, and snapshot slot.
///
/// `enforcement_domains.external_physical`: "every worktree, staging, snapshot,
/// and container intent is a durable per-owner recovery record in its row,
/// reclaimed at process start (never 'empty')".
///
/// The worktree path is **not** a field. Reclaim derives it from the intent's
/// own name and the execution root, so a record cannot name a path outside the
/// root it lives in — the containment `cleanup` requires ("expected-path,
/// contained, idempotent, and never establishes authority") is then structural
/// rather than checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentRecord {
    /// `task`, `staging`, or `snapshot`.
    pub kind: String,
    /// The slot's path relative to the execution root, as Git names paths.
    pub slot: String,
    /// The run that owns it.
    pub run_id: String,
    /// The coordinator incarnation that wrote it, so a later incarnation of the
    /// same run can tell its own residue from a live sibling's.
    pub incarnation: String,
}
