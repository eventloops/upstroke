//! `WorkspaceManager` — the typed funnels for the execution root, the detached
//! worktrees, the exact snapshots, the engine refs, and the Git-object creation
//! contexts.
//!
//! `decisions.workspace_candidates.manager`: "WorkspaceManager
//! (src/workspace_manager.rs) owns execution-root derivation and containment,
//! detached linked worktrees with durable synced intents (tasks/k<key>-g<gen>,
//! merge/s<seq>), exact snapshot worktrees with intents, engine refs, byte-safe
//! changed-path capture, worktree quiescence verification, object-residue
//! classification, and forced removal; the user's checkout is read only for
//! base capture; every worktree, snapshot, ref, pin, Git object, lock,
//! reservation, container start, event-log open or append, and run-directory
//! write goes through typed funnel APIs that take a typed site".
//!
//! # What a funnel is here
//!
//! `decisions.effect_site_inventory.identity`: "every effectful funnel API
//! takes its group's site by value, and the funnel itself calls
//! `hook(Before, site) -> primitive -> hook(After, site)`, so hooks exist for
//! every site by construction". [`funnel`] is that sentence, once, and every
//! primitive in this module goes through it. Production passes [`NoHooks`],
//! which answers [`Injection::Proceed`](crate::topology::effects::Injection::Proceed)
//! and records nothing; the ST-07 subset passes [`HarnessEffects`], which records
//! into PR3's [`HookHarness`](crate::topology::effects::HookHarness).
//!
//! The after hook is **not** called when the primitive returned `Err`. The
//! after phase's claim is `AfterEffect::Referenced` / `Unreferenced` /
//! `Released` — "the artifact is present and referenced by the row `row()`
//! names" — and a funnel that ran it after a failed primitive would record an
//! execution of a phase whose claim is false, which is the same false report
//! [`HookHarness`](crate::topology::effects::HookHarness) exists to prevent.
//!
//! # Nothing here is a production caller
//!
//! `slice_contract.non_goals[0]` is "production topology callers", and
//! `production_effect` is "none in behavior". These primitives are reached by
//! the suite and by gate evidence; the schema-4 coordinator that will call them
//! arrives in PR7–PR10. That is why this module adds no call site to
//! `src/engine/**` and changes no existing behaviour.
//!
//! # The reading trap of the packet, applied once here
//!
//! Every sentence quoted in this module comes from `decisions.*`, `invariants`,
//! or `transaction_fault_matrix`. `*_verification_dispositions`,
//! `finding_dispositions[].rationale` and the `v4_`..`v15_` keys are the
//! packet's disposition history and are quoted nowhere.
//!
//! # Allowlist placement
//!
//! `decisions.effect_site_inventory.mechanism` names this file first in the
//! **funnel section** of `effects/allowlist.toml`: "funnel modules
//! (src/workspace_manager.rs, …) each reviewed to perform effects only inside
//! site-taking APIs and never to return writable handles". Both halves of that
//! review are structural here: every effect is issued inside a [`funnel`] call
//! that takes an [`EffectSiteId`] by value, and no public function returns a
//! `File`, an `OpenOptions`, or a `Command` — the only handles that leave this
//! module are paths, object ids, and values.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, HookPhase, ObjectSite, RefSite, ResourceRow, SnapshotSite, SubEffectPoint,
    WorktreeSite,
};
use crate::topology::paths::PathSet;
use crate::util::{DurabilityLedger, DurableStep};

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

mod hooks;
pub use self::hooks::{EffectHooks, HarnessEffects, NoHooks};
use self::hooks::{apply, funnel, point};

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// The runtime refusals this module owns, as values.
///
/// A variant rather than a message so a test pins the *reason* rather than a
/// substring: `expected_failures_refusals` names six runtime refusals in this
/// lane's scope and a suite that matched on prose would pass when the wrong one
/// fired.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    /// `expected_failures_refusals[0]`, and
    /// `transaction_fault_matrix[T-DISPATCH].refusal_condition`: "worktree path
    /// outside execution root **or on a reparse point**".
    #[error(
        "refusing {}: `{}` on the chain is a symlink or reparse point, and DESIGN.md §15 creates an \
         execution root only when the chain from the authorized private root carries none",
        .chain.display(),
        .at.display()
    )]
    ReparsePointOnChain {
        /// The path whose chain was walked.
        chain: PathBuf,
        /// The component that is a symlink, junction, or other reparse point.
        at: PathBuf,
    },

    /// `DESIGN.md` §15 places the execution root at
    /// `<private root>/workspaces/<repo-key>/<run-id>`, recorded exactly. The
    /// reparse-point walk is anchored at the authorized private root and
    /// inspects the chain **below** it, one plain component at a time. A root
    /// that does not lie below the private root as plain components — no
    /// common prefix, or a prefix, a root or `..` in the remainder — has no
    /// such chain, and the walk refuses it rather than answer "no reparse
    /// point" for a chain it never inspected. [`Refusal::RunId`] refuses the
    /// run ids that would build such a root before any path exists; this is
    /// the walk's own guarantee behind that one.
    #[error(
        "refusing execution root {}: it does not lie below the authorized private root {} as a \
         chain of plain components, and DESIGN.md §15 places every execution root at \
         <private root>/workspaces/<repo-key>/<run-id>",
        .root.display(),
        .private_root.display()
    )]
    RootOutsidePrivateRoot {
        /// The candidate execution root.
        root: PathBuf,
        /// The authorized private root the walk is anchored at.
        private_root: PathBuf,
    },

    /// `execution_root`: "the canonical root is inside no repository worktree".
    #[error(
        "refusing execution root {}: it is inside the repository worktree {}",
        .root.display(),
        .worktree.display()
    )]
    RootInsideRepositoryWorktree {
        /// The candidate execution root.
        root: PathBuf,
        /// The worktree that contains it.
        worktree: PathBuf,
    },

    /// `execution_root`: "and no repository worktree is inside it".
    #[error(
        "refusing execution root {}: the repository worktree {} is inside it and is not one this \
         manager registered",
        .root.display(),
        .worktree.display()
    )]
    WorktreeInsideRoot {
        /// The candidate execution root.
        root: PathBuf,
        /// The foreign worktree inside it.
        worktree: PathBuf,
    },

    /// `transaction_fault_matrix[T-SCRUB].refusal_condition`: "path outside
    /// execution root". Also `cleanup`: "cleanup is expected-path, contained,
    /// idempotent, and never establishes authority".
    #[error(
        "refusing to touch {}: it is outside the execution root {}",
        .path.display(),
        .root.display()
    )]
    PathOutsideExecutionRoot {
        /// The execution root.
        root: PathBuf,
        /// The path that is not inside it.
        path: PathBuf,
    },

    /// `execution_root`: "created only when the managed base is a real
    /// directory".
    #[error("refusing to manage {}: the managed base is not a real directory", .path.display())]
    BaseIsNotADirectory {
        /// The base that was offered.
        path: PathBuf,
    },

    /// `ref_rules`: "symbolic refs refused". `INV-17`.
    #[error(
        "refusing to touch `{refname}`: it is a symbolic ref pointing at `{target}`, and \
         INV-17 makes every engine ref direct"
    )]
    SymbolicRef {
        /// The ref that was to be created, moved, or deleted.
        refname: String,
        /// What it points at.
        target: String,
    },

    /// `expected_failures_refusals[4]`: "checked-out integration ref".
    /// `integration_ref`: "never checked out".
    #[error(
        "refusing to publish `{refname}`: it is checked out in the worktree {}, and \
         decisions.workspace_candidates.integration_ref says the integration ref is never checked \
         out",
        .worktree.display()
    )]
    CheckedOutRef {
        /// The ref.
        refname: String,
        /// The worktree that has it checked out.
        worktree: PathBuf,
    },

    /// `expected_failures_refusals[2]`: "unexpected refs under the run
    /// namespace". `transaction_fault_matrix[T-CAND-OBJ].refusal_condition`:
    /// "pin symbolic or an unexpected ref under the run namespace".
    #[error("refusing the run namespace `{namespace}`: it carries the unexpected ref `{refname}`")]
    UnexpectedRefUnderNamespace {
        /// The namespace that was censused.
        namespace: String,
        /// The ref that nothing expected.
        refname: String,
    },

    /// `INV-17`: "moved or deleted only **expected-old**".
    ///
    /// Measured, git 2.43: `git update-ref --no-deref -d <ref>
    /// 0000000000000000000000000000000000000000` **succeeds and deletes the
    /// ref**, because the null object id means "must not exist" rather than
    /// "must be this". A caller that reached this primitive with a recorded
    /// value it had never filled in would therefore perform an *unconditional*
    /// delete through an API whose whole contract is that it cannot. A
    /// non-null wrong value refuses correctly; only this one does not, so it is
    /// refused here.
    #[error(
        "refusing to move or delete `{refname}` against the null object id: `git update-ref` reads \
         it as \"must not exist\" and would delete unconditionally, and INV-17 makes every engine \
         ref move or delete expected-old"
    )]
    NullExpectedOld {
        /// The ref that was to be moved or deleted.
        refname: String,
    },

    /// An object id that is not a full hexadecimal id.
    #[error(
        "refusing `{value}` as the {role} object id of `{refname}`: an engine ref primitive takes \
         a full hexadecimal object id"
    )]
    MalformedObjectId {
        /// The ref.
        refname: String,
        /// Which side of the update it was.
        role: &'static str,
        /// The value as it was offered.
        value: String,
    },

    /// A slot name that is not the shape `workspace_candidates` gives it.
    /// Containment is by construction: a name that could carry a separator or
    /// `..` would put a worktree outside the execution root without any
    /// later check noticing.
    #[error("refusing the {kind} slot name `{name}`: {why}")]
    SlotName {
        /// Which slot kind.
        kind: &'static str,
        /// The name as it was offered.
        name: String,
        /// What is wrong with it.
        why: &'static str,
    },

    /// A run id that is not one plain path component.
    ///
    /// `DESIGN.md` §15 places the execution root at
    /// `<private root>/workspaces/<repo-key>/<run-id>`, and `Path::join`
    /// would let an absolute id replace that prefix while `.`, `..` and an
    /// empty id alias the repo-key directory or a peer run's root — an
    /// absolute id naming a peer's root made that root this manager's, with
    /// the peer's worktrees as its slots. Refused before any path is built,
    /// by the rule the slot components already obey: ASCII alphanumerics,
    /// `-` and `_`, no leading `-`, which every engine-minted ULID satisfies.
    #[error(
        "refusing the run id `{name}`: {why}, and DESIGN.md §15 places every execution root at \
         <private root>/workspaces/<repo-key>/<run-id>"
    )]
    RunId {
        /// The id as it was offered.
        name: String,
        /// What is wrong with it.
        why: &'static str,
    },

    /// `slice_contract.invariants_introduced[1]`: "worktree and snapshot
    /// intents **synced before** the add".
    ///
    /// The two are separate sites — the cancellation clause is per clause, and
    /// `WriteIntent` and `Add` each carry their own hooks — so the ordering
    /// cannot be a single funnel body. It is enforced here instead: an add
    /// whose intent is not already durable would create a worktree that
    /// [`WorkspaceManager::reclaim_intents`] can never find, which is exactly
    /// the leak `enforcement_domains.external_physical` writes the intent to
    /// prevent ("a durable per-owner recovery record in its row, reclaimed at
    /// process start (never 'empty')").
    #[error(
        "refusing `git worktree add` for `{slot}`: its durable intent {} does not exist, and \
         the intent is synced before the add so that an interrupted add is always reclaimable",
        .intent.display()
    )]
    AddWithoutIntent {
        /// The slot whose add was refused.
        slot: String,
        /// Where its intent was looked for.
        intent: PathBuf,
    },
}

impl From<Refusal> for UpstrokeError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused {
            message: refusal.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// R18: the repository key and the execution root
// ---------------------------------------------------------------------------

/// The domain-separation prefix of `repo_key` v1.
///
/// `decisions.workspace_candidates.execution_root`: "repo_key v1 =
/// hex16(sha256('upstroke-repo-key-v1' NUL canonical common git dir bytes))".
const REPO_KEY_V1_DOMAIN: &[u8] = b"upstroke-repo-key-v1";

/// How many hex characters `hex16` keeps.
///
/// Read as sixteen hex *characters* — eight bytes of the digest. The other
/// reading, sixteen bytes rendered as thirty-two characters, is available and
/// is not what "hex16" says: the value is a directory component in
/// `<private_root>/workspaces/<repo_key>/<run_id>`, and every other short
/// digest this project renders (`invocation`'s hash) is named for the character
/// count it produces.
const REPO_KEY_HEX_CHARS: usize = 16;

/// `hex16(sha256(...))` of `decisions.workspace_candidates.execution_root`.
#[must_use]
pub fn repo_key_v1(canonical_common_git_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(REPO_KEY_V1_DOMAIN);
    hasher.update([0u8]);
    hasher.update(canonical_common_git_dir.as_os_str().as_encoded_bytes());
    let digest = hasher.finalize();
    let mut key = String::with_capacity(REPO_KEY_HEX_CHARS);
    for byte in digest.iter().take(REPO_KEY_HEX_CHARS.div_ceil(2)) {
        use std::fmt::Write as _;
        let _ = write!(key, "{byte:02x}");
    }
    key.truncate(REPO_KEY_HEX_CHARS);
    key
}

/// `<private_root>/workspaces/<repo_key>/<run_id>`, recorded exactly.
///
/// `run_id` is one plain component: [`WorkspaceManager::derive`] refuses any
/// other shape with [`refuse_unplain_run_id`] before calling this, because
/// `join` would let an absolute id replace the prefix and `.` or `..` name
/// another directory.
#[must_use]
pub fn execution_root_of(private_root: &Path, repo_key: &str, run_id: &str) -> PathBuf {
    private_root.join("workspaces").join(repo_key).join(run_id)
}

/// [`Refusal::RunId`] unless `run_id` is one plain path component.
///
/// The rule is `naming::safe_component`'s, restated for a run id so the
/// refusal names what was offered: non-empty, ASCII alphanumerics, `-` and
/// `_` only, no leading `-`. That excludes every separator on every
/// platform, `.`, `..`, a prefix such as `C:` and the trailing dot or space
/// Win32 rewrites. It is a restatement rather than a call because
/// `safe_component` is being reshaped into a `Result` carrying the same
/// messages on another branch; the two fold into one helper in the parent's
/// own sweep (`src/workspace_manager.rs` in `standards/SWEEP.md`'s queue),
/// and until then
/// `a_run_id_and_a_slot_component_are_refused_by_the_same_rule` holds them
/// to the same verdicts so the restatement cannot drift.
fn refuse_unplain_run_id(run_id: &str) -> Result<(), Refusal> {
    let why = if run_id.is_empty() {
        "it is empty"
    } else if !run_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        "only ASCII alphanumerics, `-` and `_` are legal in a run id"
    } else if run_id.starts_with('-') {
        "a leading `-` would be read as an option by the Git commands the funnels run"
    } else {
        return Ok(());
    };
    Err(Refusal::RunId {
        name: run_id.to_owned(),
        why,
    })
}

// ---------------------------------------------------------------------------
// Path hygiene
// ---------------------------------------------------------------------------

mod containment;
use self::containment::{
    canonical_prefix, is_at_or_inside, refuse_reparse_points, refuse_unreal_directory,
    strip_verbatim,
};

// ---------------------------------------------------------------------------
// Slots: the worktree, staging, and snapshot names the packet gives
// ---------------------------------------------------------------------------

mod naming;
use self::naming::safe_component;
pub use self::naming::{IntentRecord, Slot, SnapshotName};

/// The slot's effect-site vocabulary: which [`EffectSiteId`] each of its four
/// funnel positions runs under, and the [`ResourceRow`] that accounts for it.
///
/// Kept in this file rather than in `naming` with the rest of [`Slot`]
/// because these five methods are the only place eleven of the inventory's
/// sites are named as literals, and
/// `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
/// reads `src/workspace_manager.rs` **by path** to check that a funnel module
/// names every site it owns. A split that moved them into a child would leave
/// that census reading a file the names had left, so it would report eleven
/// sites as having no funnel at all — while the funnels themselves had not
/// moved an inch. The child keeps the pure name arithmetic; the site mapping
/// belongs to the funnels, and the funnels are here.
impl Slot {
    /// The row that accounts for this slot.
    ///
    /// Taken from the frozen site enums rather than restated: `R9`, `R10` and
    /// `R24` are what `WorktreeSite::Add.row()`, `AddStaging.row()` and
    /// `SnapshotSite::Add.row()` already answer.
    #[must_use]
    pub fn row(&self) -> ResourceRow {
        self.add_site().row()
    }

    /// The site the slot's `git worktree add` runs under.
    #[must_use]
    pub fn add_site(&self) -> EffectSiteId {
        match self {
            Self::Task { .. } => EffectSiteId::Worktree(WorktreeSite::Add),
            Self::Staging { .. } => EffectSiteId::Worktree(WorktreeSite::AddStaging),
            Self::Snapshot { .. } => EffectSiteId::Snapshot(SnapshotSite::Add),
        }
    }

    /// The site the slot's intent is written under.
    #[must_use]
    pub fn write_intent_site(&self) -> EffectSiteId {
        match self {
            Self::Task { .. } => EffectSiteId::Worktree(WorktreeSite::WriteIntent),
            Self::Staging { .. } => EffectSiteId::Worktree(WorktreeSite::WriteStagingIntent),
            Self::Snapshot { .. } => EffectSiteId::Snapshot(SnapshotSite::WriteIntent),
        }
    }

    /// The site the slot's forced removal runs under.
    #[must_use]
    pub fn remove_site(&self) -> EffectSiteId {
        match self {
            Self::Task { .. } => EffectSiteId::Worktree(WorktreeSite::Remove),
            Self::Staging { .. } => EffectSiteId::Worktree(WorktreeSite::RemoveStaging),
            Self::Snapshot { .. } => EffectSiteId::Snapshot(SnapshotSite::Remove),
        }
    }

    /// The site the slot's intent removal runs under.
    #[must_use]
    pub fn remove_intent_site(&self) -> EffectSiteId {
        match self {
            Self::Task { .. } => EffectSiteId::Worktree(WorktreeSite::RemoveIntent),
            Self::Staging { .. } => EffectSiteId::Worktree(WorktreeSite::RemoveStagingIntent),
            Self::Snapshot { .. } => EffectSiteId::Snapshot(SnapshotSite::RemoveIntent),
        }
    }
}

// ---------------------------------------------------------------------------
// The manager
// ---------------------------------------------------------------------------

mod worktree;
pub use self::worktree::{Quiescence, VerifyFailure, WorktreeRecord};

/// The owner of an execution root and everything inside it.
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    base: PathBuf,
    common_git_dir: PathBuf,
    repo_key: String,
    run_id: String,
    incarnation: String,
    /// The operator's authorized private root, canonicalized. It is the anchor
    /// the reparse-point walk starts at — see `containment::reparse_point_below`.
    ///
    /// Named rather than linked: the split left that function private to the
    /// child, which is narrower than the module-wide visibility it had here, so
    /// no path from this module resolves to it and a link would be broken.
    /// Widening it to `pub(super)` to make the link work would be a visibility
    /// change made for a doc comment.
    private_root: PathBuf,
    execution_root: PathBuf,
}

/// `fs::remove_dir_all`, tolerating the window in which a just-killed process's
/// handles are still closing.
///
/// A Windows process that has exited can still hold the last references to files in
/// its worktree. The kernel answers a delete with `ERROR_SHARING_VIOLATION`, and with
/// `ERROR_ACCESS_DENIED` once a name is delete-pending — the same shape
/// `runner::container` already documents for container directories. Both clear on
/// their own in milliseconds.
///
/// This matters because the engine kills agents as ordinary control flow rather than
/// as an error path: the container runner reclaims on every cancellation, so removal
/// *races* that closure instead of meeting it occasionally. Without the retry the
/// engine reports a hard `Io` failure for a condition that was already resolving.
///
/// Unix needs none of it — unlinking detaches the name regardless of open descriptors,
/// so the first attempt succeeds — and the retry is not compiled in there. The bound
/// is deliberate: a handle held longer than `ATTEMPTS * STEP` is not a closing process,
/// and the **last attempt's** error is returned rather than masked. It is not necessarily
/// the first attempt's — a permanent ACL denial and a closing handle both answer error 5,
/// and only the passage of `ATTEMPTS * STEP` tells them apart.
///
/// **This is not `runner::container::racing_removal`, and the two must not be merged.**
/// That one resolves a *handoff*: two threads racing on one path, where the loser needs
/// only the winner's in-flight call to return, which is why it spends
/// `RACING_ACCESS_ATTEMPTS` cheap `yield_now`s and no wall-clock at all. This one waits
/// on a *kernel* condition with a millisecond timescale, which no number of yields
/// reaches. Give either race the other's budget and both stop working: the handoff
/// would sleep for a microsecond problem, and this would spin through a dead process's
/// handles long before they closed.
#[cfg(windows)]
fn remove_tree_once_handles_close(path: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};

    const ATTEMPTS: u32 = 40;
    const STEP: std::time::Duration = std::time::Duration::from_millis(25);

    let mut attempt = 1_u32;
    loop {
        let error = match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            // The path is gone, which for a *removal* is the requested outcome.
            // On Windows this is exactly how a delete-pending name resolves: an
            // earlier attempt answered `ERROR_ACCESS_DENIED`, the last handle
            // closed, and the name went away — with no second actor involved, so
            // the sequence arises on its own rather than needing a race with
            // another remover. Reporting failure there would skip the Git-admin
            // cleanup below for a tree that is already deleted.
            // `runner::container::racing_removal` treats `NotFound` the same way.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => error,
        };
        let closing = matches!(
            error.raw_os_error(),
            Some(code)
                if code == ERROR_SHARING_VIOLATION as i32 || code == ERROR_ACCESS_DENIED as i32
        );
        if !closing || attempt >= ATTEMPTS {
            return Err(error);
        }
        attempt += 1;
        std::thread::sleep(STEP);
    }
}

#[cfg(not(windows))]
fn remove_tree_once_handles_close(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        // Same convergence rule as the Windows arm, so the two agree on what a
        // removal *means*. Unix reaches it only by racing another remover rather
        // than by delete-pending, but the answer is the same: the path is gone.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl WorkspaceManager {
    /// Derive the execution root of `run_id` from the managed base and the
    /// authorized private root, and refuse every containment condition
    /// `DESIGN.md` §15 names.
    ///
    /// # Errors
    ///
    /// [`Refusal::RunId`], [`Refusal::BaseIsNotADirectory`],
    /// [`Refusal::RootOutsidePrivateRoot`], [`Refusal::ReparsePointOnChain`],
    /// [`Refusal::RootInsideRepositoryWorktree`] and
    /// [`Refusal::WorktreeInsideRoot`]; [`UpstrokeError::Io`] when the base,
    /// the private root or a registered worktree cannot be read or resolved;
    /// and a Git error when the base is not a repository.
    pub fn derive(
        base: &Path,
        private_root: &Path,
        run_id: &str,
        incarnation: &str,
    ) -> Result<Self, UpstrokeError> {
        refuse_unplain_run_id(run_id)?;
        refuse_unreal_directory(base)?;
        refuse_unreal_directory(private_root)?;

        let common_git_dir = common_git_dir(base)?;
        let repo_key = repo_key_v1(&common_git_dir);
        let private_root = canonical_prefix(private_root)?;
        let execution_root = execution_root_of(&private_root, &repo_key, run_id);
        let manager = Self {
            base: canonical_prefix(base)?,
            common_git_dir,
            repo_key,
            run_id: run_id.to_owned(),
            incarnation: incarnation.to_owned(),
            private_root,
            execution_root,
        };
        manager.revalidate()?;
        Ok(manager)
    }

    /// The canonicalized authorized private root the execution root hangs from.
    #[must_use]
    pub fn private_root(&self) -> &Path {
        &self.private_root
    }

    /// The managed base checkout. Read only, for base capture.
    #[must_use]
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// The repository's canonical common git dir — the bytes `repo_key` is
    /// taken over.
    #[must_use]
    pub fn common_git_dir(&self) -> &Path {
        &self.common_git_dir
    }

    /// `repo_key` v1 of the managed repository.
    #[must_use]
    pub fn repo_key(&self) -> &str {
        &self.repo_key
    }

    /// The execution root, recorded exactly.
    #[must_use]
    pub fn execution_root(&self) -> &Path {
        &self.execution_root
    }

    /// Where a slot's worktree lives.
    #[must_use]
    pub fn slot_path(&self, slot: &Slot) -> PathBuf {
        self.execution_root.join(slot.relative())
    }

    /// Where a slot's intent lives.
    #[must_use]
    pub fn intent_path(&self, slot: &Slot) -> PathBuf {
        self.execution_root.join("intents").join(slot.intent_name())
    }

    /// The three containment conditions, re-checked.
    ///
    /// This is the **gate**, run before a funnel is entered: it refuses
    /// before any hook runs, and it is the check that asks Git for the
    /// worktree list. The chain half of it runs again *inside* every funnel
    /// primitive, immediately before the effect, as
    /// [`Self::revalidate_chain`]; that doc says why the two are separate.
    ///
    /// `execution_root`: "created only when the managed base is a real
    /// directory with no symlink/reparse point on the chain, the canonical root
    /// is inside no repository worktree, and no repository worktree is inside
    /// it; **every create/reclaim/delete revalidates**".
    ///
    /// The third clause is evaluated as *no foreign* worktree is inside it. The
    /// manager's own worktrees are inside the root by construction — that is
    /// what the root is for — so a literal reading would make the second
    /// `add` refuse. A worktree is the manager's when its path is
    /// `<root>/{tasks,merge,snapshots}/<component>`; anything else inside the
    /// root is foreign and refuses.
    ///
    /// # Errors
    ///
    /// The containment refusals, or a Git error reading the worktree list.
    pub fn revalidate(&self) -> Result<(), UpstrokeError> {
        self.revalidate_chain(&self.execution_root)?;
        let root = canonical_prefix(&self.execution_root)?;
        for record in self.worktree_records()? {
            let worktree = canonical_prefix(&record.path)?;
            if is_at_or_inside(&worktree, &root) {
                return Err(Refusal::RootInsideRepositoryWorktree {
                    root,
                    worktree: record.path,
                }
                .into());
            }
            if is_at_or_inside(&root, &worktree) && !self.is_manager_slot_path(&root, &worktree) {
                return Err(Refusal::WorktreeInsideRoot {
                    root,
                    worktree: record.path,
                }
                .into());
            }
        }
        Ok(())
    }

    /// The chain half of [`Self::revalidate`], re-run inside every funnel
    /// primitive immediately before its effect: the managed base is a real
    /// directory, the authorized private root is still the directory it was
    /// resolved as, and the chain below it **down to `below`** is plain
    /// components with no reparse point or regular file among them.
    ///
    /// `below` is the deepest path the effect acts through — the execution
    /// root, a scaffolding directory, an intent's `intents/` directory, a
    /// slot's checkout — so the walk covers the effect's own parent and not
    /// only the root. A non-recursive `remove_file` or a `create_dir_all`
    /// follows a link in its parent as readily as a link at the root, and an
    /// `intents/` exchanged for a link to a victim directory between the
    /// gate and the effect would otherwise delete or write there with every
    /// check passed. The one path every Git-running primitive acts through
    /// besides its target, `hooks-none`, is walked by the Git runner itself
    /// immediately before each command ([`Self::revalidate_hooks_path`]).
    ///
    /// `DESIGN.md` §15: every create, reclaim and delete revalidates before
    /// its funnel and re-checks the chain inside it. Between the gate and the
    /// effect sit the funnel's `Before` hook and whatever else the machine
    /// does in that window, and a private root exchanged for a link there
    /// would have every path under it resolve elsewhere with nothing left to
    /// notice — `a_registration_rebound_after_validation_keeps_its_admin_state`
    /// already drives a `Before` hook that rewrites filesystem identity. So
    /// the checks that decide *where the effect lands* run again here,
    /// adjacent to the syscall.
    ///
    /// Only these, and not the whole gate: `git worktree list` inside a
    /// primitive would make a removal depend on Git parsing the very
    /// registration that recovery exists to remove (see
    /// [`Self::revalidate_removal`]), and the worktree comparisons the gate
    /// makes need no filesystem effect to stay true. The window this leaves
    /// is the one between this check and the syscall itself: a writer that
    /// exchanges a component in that gap is not seen, and no re-check closes
    /// it. Only directory-relative syscalls close it — `openat` and
    /// `unlinkat` against a directory descriptor held from the check — and
    /// that is platform code for a later change, not this one.
    ///
    /// # Errors
    ///
    /// [`Refusal::BaseIsNotADirectory`], [`Refusal::RootOutsidePrivateRoot`],
    /// [`Refusal::ReparsePointOnChain`], or an I/O error naming the component
    /// that could not be read or is a regular file.
    fn revalidate_chain(&self, below: &Path) -> Result<(), UpstrokeError> {
        refuse_unreal_directory(&self.base)?;
        refuse_reparse_points(&self.private_root, below)
    }

    /// Whether `worktree` occupies one of this manager's own slot namespaces.
    fn is_manager_slot_path(&self, root: &Path, worktree: &Path) -> bool {
        let Ok(relative) = worktree.strip_prefix(root) else {
            return false;
        };
        let components: Vec<_> = relative.components().collect();
        if components.len() != 2 {
            return false;
        }
        let Component::Normal(namespace) = components[0] else {
            return false;
        };
        let Component::Normal(name) = components[1] else {
            return false;
        };
        matches!(
            namespace.to_str(),
            Some("tasks") | Some("merge") | Some("snapshots")
        ) && name
            .to_str()
            .is_some_and(|name| safe_component(name).is_none())
    }

    /// The slot's path, with its name validated first.
    ///
    /// Every primitive that turns a [`Slot`] into a path goes through this
    /// rather than through [`Self::slot_path`]. `Slot`'s fields are public, so
    /// the name is caller data at every entry point, not only at the two that
    /// happen to create something: `git add -A`, `git write-tree`,
    /// `git cherry-pick` and `git diff` all run with the slot path as their
    /// working directory, and a name carrying a separator would run them
    /// outside the execution root. [`Refusal::SlotName`]'s own doc comment says
    /// containment here is "by construction" — this is where that construction
    /// is applied uniformly.
    ///
    /// # Errors
    ///
    /// [`Refusal::SlotName`].
    fn slot_target(&self, slot: &Slot) -> Result<PathBuf, UpstrokeError> {
        slot.validate()?;
        Ok(self.slot_path(slot))
    }

    /// Refuse a path that is not inside the execution root.
    ///
    /// `transaction_fault_matrix[T-SCRUB].refusal_condition` is "path outside
    /// execution root", and it is the whole of what makes the forced removals
    /// safe: they delete a directory tree.
    fn contained(&self, path: &Path) -> Result<PathBuf, UpstrokeError> {
        let root = canonical_prefix(&self.execution_root)?;
        let candidate = canonical_prefix(path)?;
        if candidate == root || !candidate.starts_with(&root) {
            return Err(Refusal::PathOutsideExecutionRoot {
                root,
                path: path.to_path_buf(),
            }
            .into());
        }
        Ok(candidate)
    }

    // -----------------------------------------------------------------------
    // R18 funnels
    // -----------------------------------------------------------------------

    /// `Worktree.CreateExecutionRoot` (R18).
    ///
    /// # Errors
    ///
    /// The containment refusals, or an I/O error creating the directories.
    pub fn create_execution_root(&self, hooks: &mut dyn EffectHooks) -> Result<(), UpstrokeError> {
        self.revalidate()?;
        let ledger = hooks.durability_ledger();
        funnel(
            hooks,
            EffectSiteId::Worktree(WorktreeSite::CreateExecutionRoot),
            || {
                self.revalidate_chain(&self.execution_root)?;
                fs::create_dir_all(&self.execution_root).map_err(|source| UpstrokeError::Io {
                    path: self.execution_root.clone(),
                    source,
                })?;
                for directory in [
                    self.execution_root.join("intents"),
                    self.execution_root.join("tasks"),
                    self.execution_root.join("merge"),
                    self.execution_root.join("snapshots"),
                    self.hooks_dir(),
                ] {
                    self.revalidate_chain(&directory)?;
                    fs::create_dir_all(&directory).map_err(|source| UpstrokeError::Io {
                        path: directory,
                        source,
                    })?;
                }
                sync_directory(&self.execution_root, &ledger)
            },
        )
    }

    /// `Worktree.RemoveExecutionRoot` (R18).
    ///
    /// `resource_accounting[R18].lifecycle`: "pruned by finalization when
    /// empty; otherwise resumably_open". The answer says which happened, so a
    /// caller cannot read "did nothing" as "removed".
    ///
    /// # Errors
    ///
    /// The containment refusals, or an I/O error.
    pub fn remove_execution_root(
        &self,
        hooks: &mut dyn EffectHooks,
    ) -> Result<bool, UpstrokeError> {
        self.revalidate()?;
        funnel(
            hooks,
            EffectSiteId::Worktree(WorktreeSite::RemoveExecutionRoot),
            || {
                self.revalidate_chain(&self.execution_root)?;
                match fs::symlink_metadata(&self.execution_root) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(false);
                    }
                    Err(source) => {
                        return Err(UpstrokeError::Io {
                            path: self.execution_root.clone(),
                            source,
                        });
                    }
                }
                for scaffolding in [
                    self.hooks_dir(),
                    self.execution_root.join("intents"),
                    self.execution_root.join("tasks"),
                    self.execution_root.join("merge"),
                    self.execution_root.join("snapshots"),
                ] {
                    self.revalidate_chain(&scaffolding)?;
                    if !directory_is_empty(&scaffolding)? {
                        continue;
                    }
                    // Empty a moment ago, so a failure to remove it is a
                    // failure to report, not a race to swallow: a scaffolding
                    // directory nothing can remove is what keeps the root.
                    match fs::remove_dir(&scaffolding) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(source) => {
                            return Err(UpstrokeError::Io {
                                path: scaffolding,
                                source,
                            });
                        }
                    }
                }
                if !directory_is_empty(&self.execution_root)? {
                    return Ok(false);
                }
                fs::remove_dir(&self.execution_root).map_err(|source| UpstrokeError::Io {
                    path: self.execution_root.clone(),
                    source,
                })?;
                Ok(true)
            },
        )
    }

    /// The empty directory every funnel points `core.hooksPath` at.
    ///
    /// `decisions.workspace_candidates.candidate` calls the commit "hook-free",
    /// and a repository hook that ran inside an engine worktree would be an
    /// effect no site accounts for.
    fn hooks_dir(&self) -> PathBuf {
        self.execution_root.join("hooks-none")
    }

    // -----------------------------------------------------------------------
    // Intents (R9 / R10 / R24)
    // -----------------------------------------------------------------------

    /// `Worktree.WriteIntent` / `Worktree.WriteStagingIntent` /
    /// `Snapshot.WriteIntent`.
    ///
    /// `slice_contract.invariants_introduced[1]`: "worktree and snapshot
    /// intents **synced before add**". The record is written to a temporary,
    /// fsynced, renamed, and the directory fsynced, so an interrupted write
    /// leaves either nothing or a complete record — never a half-parsed one
    /// that reclaim would refuse.
    ///
    /// # Errors
    ///
    /// A slot refusal, the containment refusals, or an I/O error.
    pub fn write_intent(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
    ) -> Result<(), UpstrokeError> {
        slot.validate()?;
        self.revalidate()?;
        let path = self.intent_path(slot);
        // The record is a persisted schema, and its slot text is identity:
        // a validated slot is ASCII, so the checked conversion cannot fail,
        // and it is checked rather than lossy so that it never could silently.
        let relative = slot.relative();
        let slot_text = relative
            .to_str()
            .ok_or_else(|| UpstrokeError::Refused {
                message: format!(
                    "refusing to record the {} slot {}: its path is not UTF-8",
                    slot.kind(),
                    relative.display()
                ),
            })?
            .replace('\\', "/");
        let record = IntentRecord {
            kind: slot.kind().to_owned(),
            slot: slot_text,
            run_id: self.run_id.clone(),
            incarnation: self.incarnation.clone(),
        };
        let ledger = hooks.durability_ledger();
        let directory = self.execution_root.join("intents");
        funnel(hooks, slot.write_intent_site(), || {
            self.revalidate_chain(&directory)?;
            let bytes = serde_json::to_vec(&record).map_err(|error| UpstrokeError::Git {
                message: format!("serializing the {} intent: {error}", slot.kind()),
            })?;
            write_synced(&path, &bytes, &ledger)
        })
    }

    /// `Worktree.RemoveIntent` / `Worktree.RemoveStagingIntent` /
    /// `Snapshot.RemoveIntent`. Idempotent.
    ///
    /// # Errors
    ///
    /// A slot refusal, the containment refusals, or an I/O error. The name is
    /// validated here too: `intent_name` joins the slot's components with `.`
    /// into a *file name*, so an unvalidated name carrying a separator would
    /// make this `remove_file` a deletion outside the intents directory.
    pub fn remove_intent(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
    ) -> Result<(), UpstrokeError> {
        slot.validate()?;
        self.revalidate()?;
        let directory = self.execution_root.join("intents");
        let path = directory.join(slot.intent_name());
        let ledger = hooks.durability_ledger();
        funnel(hooks, slot.remove_intent_site(), || {
            self.revalidate_chain(&directory)?;
            match fs::remove_file(&path) {
                Ok(()) => sync_directory(&directory, &ledger),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(UpstrokeError::Io { path, source }),
            }
        })
    }

    /// Every intent the execution root still carries, in directory order.
    ///
    /// # Errors
    ///
    /// An I/O error, or an intent file whose name no slot renders.
    pub fn intents(&self) -> Result<Vec<Slot>, UpstrokeError> {
        let directory = self.execution_root.join("intents");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: directory,
                    source,
                });
            }
        };
        let mut slots = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| UpstrokeError::Io {
                path: directory.clone(),
                source,
            })?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| UpstrokeError::Git {
                message: format!("intent {} has a non-UTF-8 name", entry.path().display()),
            })?;
            let slot = Slot::from_intent_name(name).ok_or_else(|| UpstrokeError::Git {
                message: format!(
                    "unexpected file `{name}` in the intent directory of {}",
                    self.execution_root.display()
                ),
            })?;
            slot.validate()?;
            slots.push(slot);
        }
        slots.sort();
        Ok(slots)
    }

    /// Reclaim every intent this execution root carries: forced removal of the
    /// worktree, then the intent.
    ///
    /// `enforcement_domains.external_physical`: intents are "reclaimed at
    /// process start (never 'empty')".
    /// `transaction_fault_matrix[T-DISPATCH].resume_action` and
    /// `[T-PROPOSAL].resume_action` both remove "intent then worktree" with
    /// force, and `decisions.workspace_candidates.snapshots` says an
    /// "interrupted add leaves a registered-but-unpopulated worktree that the
    /// intent-based reclaim removes and prunes".
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git or I/O error.
    pub fn reclaim_intents(&self, hooks: &mut dyn EffectHooks) -> Result<Vec<Slot>, UpstrokeError> {
        let slots = self.intents()?;
        // Revalidate even when there are no intents: callers rely on reclaim
        // as a fresh containment check. With nothing to remove, Git's ordinary
        // enumeration is safe and a repository with no linked-worktree store
        // is an ordinary empty state. Every non-empty case is revalidated by
        // `remove_worktree`, where a missing store must refuse before deletion.
        if slots.is_empty() {
            self.revalidate()?;
        }
        for slot in &slots {
            self.remove_worktree(hooks, slot)?;
            self.remove_intent(hooks, slot)?;
        }
        Ok(slots)
    }

    // -----------------------------------------------------------------------
    // Worktree and snapshot funnels (R9 / R10 / R24)
    // -----------------------------------------------------------------------

    /// The fixed argv of the four Git commands the residue kill sampler drives
    /// (Fable's `PR5-CONF-004`).
    ///
    /// `command_internal_sub_effects` (ii) is "real-command kill sampling — the
    /// Git child of the site is killed at uncontrolled points **through the
    /// process funnel** across N runs". The sampler spawns its own `git` child
    /// with an argv it transcribed from these funnels. The transcription was
    /// faithful, and nothing made it stay faithful: changing a funnel's argv —
    /// adding a flag to the stage, say — would leave the sampler silently
    /// sampling a stale command with every assertion green, and the
    /// recovery-proven evidence would no longer describe the funnel's real
    /// child.
    ///
    /// So the transcription is gone. There is one list per command, the funnel
    /// and the sampler both read it, and
    /// `no_sampled_funnel_builds_its_argv_from_a_literal` fails if a funnel
    /// grows an argument that does not come through here. It does **not** make
    /// the kill go through the process funnel — that is
    /// `PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` in `reviews/FINDINGS.md`, owned by
    /// PR6/PR7 with `src/runner/**` frozen — and this comment does not claim it
    /// does.
    pub(crate) const CANDIDATE_STAGE_ARGV: [&str; 4] = ["add", "-A", "--", "."];
    /// See [`Self::CANDIDATE_STAGE_ARGV`]. Takes no dynamic argument.
    pub(crate) const CANDIDATE_WRITE_TREE_ARGV: [&str; 1] = ["write-tree"];
    /// See [`Self::CANDIDATE_STAGE_ARGV`]. Takes the commit to pick.
    pub(crate) const PROPOSAL_CHERRY_PICK_ARGV: [&str; 1] = ["cherry-pick"];
    /// See [`Self::CANDIDATE_STAGE_ARGV`]. Takes the path and the commit.
    pub(crate) const WORKTREE_ADD_ARGV: [&str; 4] = ["worktree", "add", "--detach", "--quiet"];

    /// `Worktree.Add` / `Worktree.AddStaging` / `Snapshot.Add`: a **detached**
    /// linked worktree at `commit`.
    ///
    /// The intent must already be durable, and this funnel **refuses** if it is
    /// not. `write_intent` is a separate site rather than a step inside this
    /// one because the cancellation clause is per clause: "an interrupted
    /// worktree or snapshot add leaves a durable intent that reclaim removes".
    /// Separate sites make the *ordering* a caller's obligation, so the
    /// obligation is checked here — see [`Refusal::AddWithoutIntent`].
    ///
    /// # Errors
    ///
    /// A slot refusal, [`Refusal::AddWithoutIntent`], the containment refusals,
    /// or a Git error.
    pub fn add_worktree(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        commit: &str,
    ) -> Result<PathBuf, UpstrokeError> {
        let path = self.slot_target(slot)?;
        self.revalidate()?;
        let intent = self.intent_path(slot);
        funnel(hooks, slot.add_site(), move || {
            self.revalidate_chain(&path)?;
            // Inside the funnel, after the `Before` hook: an intent removed
            // between a check outside and the add would leave a worktree that
            // `reclaim_intents` can never find. Absent is the refusal;
            // anything that is not a regular file is the same refusal, since
            // only a file is a durable record; a metadata failure — a loop
            // planted at the intent's name, permission — is an error, not
            // "no intent".
            let durable = match fs::symlink_metadata(&intent) {
                Ok(metadata) => metadata.is_file(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(source) => {
                    return Err(UpstrokeError::Io {
                        path: intent,
                        source,
                    });
                }
            };
            if !durable {
                return Err(Refusal::AddWithoutIntent {
                    slot: slot.relative().display().to_string(),
                    intent,
                }
                .into());
            }
            // Inside the funnel, not before it (`PR5-CONF-003`). `identity` says
            // "the funnel itself calls hook(Before, site) -> primitive ->
            // hook(After, site)" and `scope` requires "every effect through
            // typed funnel APIs taking a site"; this scaffolding `create_dir_all`
            // sat outside the call, so a hook armed to refuse at
            // `Before(Worktree.Add)` returned its refusal *after* the directory
            // had already been created. Measured: against a slot whose
            // scaffolding directory was removed, the refusal arrived and the
            // directory existed.
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| UpstrokeError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let mut argv: Vec<OsString> =
                Self::WORKTREE_ADD_ARGV.iter().map(OsString::from).collect();
            argv.push(path.as_os_str().to_os_string());
            argv.push(OsString::from(commit));
            self.git_ok(&self.base, &argv)?;
            Ok(path)
        })
    }

    /// `Worktree.Verify` — the read-only quiescence observation.
    ///
    /// The site is `is_read_only()`, so it performs nothing at either phase;
    /// its hooks still fire, because ST-07 requires every site observed
    /// executed and a read-only site is still a site.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error. A worktree that is *not*
    /// quiescent is `Ok(Err(VerifyFailure))`, not an error: its failure routes
    /// to forced removal and a fresh add, which is a decision the caller makes.
    pub fn verify_worktree(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        expected: &Quiescence,
    ) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        funnel(hooks, EffectSiteId::Worktree(WorktreeSite::Verify), || {
            self.revalidate_chain(&path)?;
            self.quiescence(&path, expected)
        })
    }

    /// The body of [`Self::verify_worktree`], so the sampling harness can ask
    /// the same question without a second hook execution.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn quiescence(
        &self,
        path: &Path,
        expected: &Quiescence,
    ) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
        let Some(record) = self.worktree_record(path)? else {
            return Ok(Err(VerifyFailure::NotRegistered));
        };
        if record.locked.as_deref() == Some("initializing") {
            return Ok(Err(VerifyFailure::Unpopulated));
        }
        if !path.is_dir() {
            return Ok(Err(VerifyFailure::Missing));
        }
        let Some(git_dir) = self.worktree_git_dir(path)? else {
            return Ok(Err(VerifyFailure::Missing));
        };
        match common_git_dir(path) {
            Ok(common) if common == self.common_git_dir => {}
            Ok(_) => return Ok(Err(VerifyFailure::ForeignRepository)),
            Err(_) => return Ok(Err(VerifyFailure::Missing)),
        }
        if let Some(element) = administrative_residue_at(&git_dir)?.first() {
            return Ok(Err(VerifyFailure::Residue(*element)));
        }
        match expected {
            Quiescence::AtBase(base) => {
                let head = self.git_line(path, &["rev-parse", "HEAD"])?;
                if !head.eq_ignore_ascii_case(base) {
                    return Ok(Err(VerifyFailure::HeadMismatch {
                        expected: base.clone(),
                        actual: head,
                    }));
                }
            }
            Quiescence::HoldsTree(tree) => {
                // Read-only, and now literally (`PR5-CONF-002`). This ran
                // `git write-tree`, under a comment claiming it "creates no
                // object that is not already implied by the index it reads" —
                // and "implied by" is not "already present". Measured against
                // git 2.43.0: an index carrying staged content whose tree object
                // was never written gains **two loose objects**, and the index's
                // own bytes are rewritten 104 → 165 with the `TREE` cache-tree
                // extension added. That reachable prefix is exactly the one
                // `Object.CandidateStage` leaves before `Object.CandidateWriteTree`
                // runs. `identity` calls `Worktree.Verify` "a read-only
                // quiescence observation (no effect)" and
                // `WorktreeSite::Verify::is_read_only()` lives in a frozen file,
                // so the code is what had to move.
                if let Some(difference) = self.index_differs_from(path, tree)? {
                    return Ok(Err(VerifyFailure::TreeMismatch {
                        expected: tree.clone(),
                        difference,
                    }));
                }
            }
        }
        Ok(Ok(()))
    }

    /// How the worktree's **index** differs from `tree`, or `None` when it holds
    /// exactly that tree — computed **without writing anything**
    /// (`PR5-CONF-002`).
    ///
    /// `diff-index --cached` asks the question `write-tree` was being used to
    /// answer — *does the index hold this exact tree* — and answers it by
    /// reading. `--no-optional-locks` is what makes that read-only rather than
    /// nearly: without it `diff-index` takes the index lock to write back a
    /// refreshed stat cache, which is a write to `.git/index`.
    ///
    /// Three outcomes, because `--quiet` implies `--exit-code`: 0 is "holds it",
    /// 1 is "differs", and anything else is a Git failure — of which one case is
    /// ordinary rather than exceptional and is answered rather than propagated:
    /// a recorded tree that is not an object in this repository at all. A
    /// worktree cannot hold a tree the repository does not have, so that is a
    /// mismatch, which is also what the pre-repair code reported for it.
    ///
    /// # Errors
    ///
    /// A Git error other than "the index differs" or "the tree is absent".
    fn index_differs_from(&self, path: &Path, tree: &str) -> Result<Option<String>, UpstrokeError> {
        const READ_ONLY: &str = "--no-optional-locks";
        let quiet = read_only_git(
            path,
            &[READ_ONLY, "diff-index", "--cached", "--quiet", tree, "--"],
        )?;
        match quiet.status.code() {
            Some(0) => return Ok(None),
            Some(1) => {}
            _ => {
                let present = read_only_git(
                    path,
                    &[READ_ONLY, "cat-file", "-e", &format!("{tree}^{{tree}}")],
                )?;
                if present.status.success() {
                    return Err(UpstrokeError::Git {
                        message: format!(
                            "git diff-index against {tree} failed in {}: {}",
                            path.display(),
                            String::from_utf8_lossy(&quiet.stderr).trim()
                        ),
                    });
                }
                return Ok(Some(
                    "that tree is not an object in this repository".to_owned(),
                ));
            }
        }

        // NUL-separated, because a path may contain a newline and a diagnostic
        // that split on one would name paths that do not exist.
        let names = read_only_git_ok(
            path,
            &[
                READ_ONLY,
                "diff-index",
                "--cached",
                "--name-only",
                "-z",
                tree,
                "--",
            ],
        )?;
        let differing: Vec<String> = String::from_utf8_lossy(&names)
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        let listed: Vec<&String> = differing.iter().take(8).collect();
        let more = differing.len().saturating_sub(listed.len());
        let mut message = format!(
            "{} path(s) differ: {}",
            differing.len(),
            listed
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        if more > 0 {
            message.push_str(&format!(" and {more} more"));
        }
        Ok(Some(message))
    }

    /// `Worktree.Remove` / `Worktree.RemoveStaging` / `Snapshot.Remove` —
    /// **forced**, and idempotent.
    ///
    /// `decisions.workspace_candidates.cleanup`: "every worktree, staging, and
    /// snapshot removal is forced (`git worktree remove --force` semantics, or
    /// contained expected-path deletion followed by `git worktree prune`) so
    /// Git administrative residue left by an interrupted command (index.lock,
    /// CHERRY_PICK_HEAD, MERGE_HEAD, MERGE_MSG, ORIG_HEAD, sequencer state, **a
    /// registered-but-unpopulated worktree**) never blocks reclaim".
    ///
    /// The contained-deletion form is the one implemented, because it is the
    /// only one that works when the checkout is already gone — and because it
    /// is the form whose containment is checkable. The `locked` marker
    /// `git worktree add` leaves behind is cleared as part of the removal:
    /// measured, `git worktree prune` skips a locked entry and
    /// `git worktree remove --force` refuses one, so a removal that did not
    /// clear it would leave exactly the residue this sentence promises never
    /// blocks reclaim.
    ///
    /// # Errors
    ///
    /// The containment refusals, or a Git or I/O error.
    pub fn remove_worktree(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
    ) -> Result<(), UpstrokeError> {
        let path = self.slot_target(slot)?;
        let registration = self.revalidate_removal(&path)?;
        funnel(hooks, slot.remove_site(), || {
            self.revalidate_chain(&path)?;
            let present = match fs::symlink_metadata(&path) {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(source) => {
                    return Err(UpstrokeError::Io {
                        path: path.clone(),
                        source,
                    });
                }
            };
            if present {
                let contained = self.contained(&path)?;
                remove_tree_once_handles_close(&contained).map_err(|source| UpstrokeError::Io {
                    path: contained,
                    source,
                })?;
            }
            if let Some(admin) = registration.as_ref() {
                if !self.registration_still_names(admin, &path)? {
                    // Its identity metadata is already absent: forced cleanup
                    // converges without inferring or deleting an admin path.
                    self.git_ok(
                        &self.base,
                        &[OsString::from("worktree"), OsString::from("prune")],
                    )?;
                    return Ok(());
                }
                let locked = admin.join("locked");
                match fs::remove_file(&locked) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(UpstrokeError::Io {
                            path: locked,
                            source,
                        });
                    }
                }
                // A killed `git worktree add` can leave an empty `commondir`.
                // Git then cannot enumerate *any* worktree, so `prune` cannot
                // remove this one. `revalidate_removal` bound this admin
                // directory to the exact, contained slot from its byte-safe
                // `gitdir` before the checkout was deleted. Only that proved
                // registration may be removed directly.
                let commondir = admin.join("commondir");
                let commondir_empty = match fs::metadata(&commondir) {
                    Ok(metadata) => metadata.len() == 0,
                    // No `commondir` at all is Git's to prune; only a read
                    // failure is ours to report.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(source) => {
                        return Err(UpstrokeError::Io {
                            path: commondir,
                            source,
                        });
                    }
                };
                if commondir_empty {
                    if !self.registration_still_names(admin, &path)? {
                        self.git_ok(
                            &self.base,
                            &[OsString::from("worktree"), OsString::from("prune")],
                        )?;
                        return Ok(());
                    }
                    remove_tree_once_handles_close(admin).map_err(|source| UpstrokeError::Io {
                        path: admin.clone(),
                        source,
                    })?;
                }
            }
            self.git_ok(
                &self.base,
                &[OsString::from("worktree"), OsString::from("prune")],
            )?;
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // The exact snapshot store (R24)
    // -----------------------------------------------------------------------

    /// Add an exact snapshot: a detached checkout of exactly the tree under
    /// judgment.
    ///
    /// `decisions.workspace_candidates.snapshots`: "for a tree-only candidate
    /// input the snapshot funnel first creates an ephemeral commit of that tree
    /// on the recorded parent (Object.SnapshotCommitTree: unreferenced, R27,
    /// until the worktree add makes it the snapshot HEAD, R24 …), while
    /// integration snapshots check out the proposal or head commit and create
    /// no object; **intent synced before `git worktree add`**".
    ///
    /// The order is therefore: commit-tree (when the input is a tree) → intent
    /// → add. That is also the order the cancellation clause depends on — "an
    /// ephemeral snapshot commit created *before* the intent is left to Git" —
    /// so the object exists before anything durable claims it.
    ///
    /// # Errors
    ///
    /// A slot refusal, the containment refusals, or a Git error.
    pub fn add_snapshot(
        &self,
        hooks: &mut dyn EffectHooks,
        name: &SnapshotName,
        input: &SnapshotInput,
    ) -> Result<Snapshot, UpstrokeError> {
        let slot = Slot::Snapshot { name: name.clone() };
        let (head, ephemeral) = match input {
            SnapshotInput::Commit(commit) => (commit.clone(), None),
            SnapshotInput::Tree { tree, parent } => {
                let commit = self.snapshot_commit_tree(hooks, tree, parent)?;
                (commit.clone(), Some(commit))
            }
        };
        self.write_intent(hooks, &slot)?;
        let path = self.add_worktree(hooks, &slot, &head)?;
        Ok(Snapshot {
            slot,
            path,
            head,
            ephemeral,
        })
    }

    /// Remove an exact snapshot: forced worktree removal, then the intent.
    ///
    /// # Errors
    ///
    /// The containment refusals, or a Git or I/O error.
    pub fn remove_snapshot(
        &self,
        hooks: &mut dyn EffectHooks,
        snapshot: &Snapshot,
    ) -> Result<(), UpstrokeError> {
        self.remove_worktree(hooks, &snapshot.slot)?;
        self.remove_intent(hooks, &snapshot.slot)
    }

    // -----------------------------------------------------------------------
    // Ref primitives (R11 / R12 / R21 / R23) — INV-17
    // -----------------------------------------------------------------------

    /// `Ref.*` creation, zero-old and `--no-deref`.
    ///
    /// `ref_rules`: "all refs direct, created zero-old with `--no-deref`, moved
    /// or deleted only expected-old; symbolic refs refused".
    ///
    /// # Errors
    ///
    /// [`Refusal::SymbolicRef`], or a Git error — including the zero-old
    /// failure when the ref already exists.
    pub fn create_ref_zero_old(
        &self,
        hooks: &mut dyn EffectHooks,
        site: RefSite,
        refname: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        self.refuse_symbolic(refname)?;
        refuse_malformed_object_id(refname, "new", new)?;
        funnel(hooks, EffectSiteId::Ref(site), || {
            self.update_ref(&["--no-deref", refname, new, ""])
        })
    }

    /// `Ref.CompareAndSwapIntegration`: expected-old, `--no-deref`.
    ///
    /// # Errors
    ///
    /// [`Refusal::SymbolicRef`], [`Refusal::CheckedOutRef`], or a Git error
    /// when the old value does not match.
    pub fn compare_and_swap_ref(
        &self,
        hooks: &mut dyn EffectHooks,
        site: RefSite,
        refname: &str,
        old: &str,
        new: &str,
    ) -> Result<(), UpstrokeError> {
        self.assert_publishable(refname)?;
        refuse_malformed_object_id(refname, "new", new)?;
        refuse_expected_old(refname, old)?;
        funnel(hooks, EffectSiteId::Ref(site), || {
            self.update_ref(&["--no-deref", refname, new, old])
        })
    }

    /// `Ref.Delete*` / pin pruning: expected-old, `--no-deref`.
    ///
    /// # Errors
    ///
    /// [`Refusal::SymbolicRef`] or a Git error when the old value does not
    /// match.
    pub fn delete_ref_expected_old(
        &self,
        hooks: &mut dyn EffectHooks,
        site: RefSite,
        refname: &str,
        old: &str,
    ) -> Result<(), UpstrokeError> {
        self.refuse_symbolic(refname)?;
        refuse_expected_old(refname, old)?;
        funnel(hooks, EffectSiteId::Ref(site), || {
            self.update_ref(&["--no-deref", "-d", refname, old])
        })
    }

    /// `assert_publishable()` of `decisions.workspace_candidates.integration_ref`
    /// — "before every prepare/CAS/recovery".
    ///
    /// # Errors
    ///
    /// [`Refusal::SymbolicRef`] or [`Refusal::CheckedOutRef`].
    pub fn assert_publishable(&self, refname: &str) -> Result<(), UpstrokeError> {
        self.refuse_symbolic(refname)?;
        for record in self.worktree_records()? {
            if record.branch.as_deref() == Some(refname) {
                return Err(Refusal::CheckedOutRef {
                    refname: refname.to_owned(),
                    worktree: record.path,
                }
                .into());
            }
        }
        Ok(())
    }

    /// The direct target of `refname`, or `None` when nothing is there.
    ///
    /// # Errors
    ///
    /// [`Refusal::SymbolicRef`], or a Git error.
    pub fn direct_ref_target(&self, refname: &str) -> Result<Option<String>, UpstrokeError> {
        self.refuse_symbolic(refname)?;
        let output = self.git(
            &self.base,
            &[
                OsString::from("show-ref"),
                OsString::from("--verify"),
                OsString::from("--"),
                OsString::from(refname),
            ],
        )?;
        if !output.status.success() {
            return Ok(None);
        }
        let line = String::from_utf8_lossy(&output.stdout);
        Ok(line
            .split_whitespace()
            .next()
            .map(std::borrow::ToOwned::to_owned))
    }

    /// Every ref under `namespace`, as `(refname, object id)`.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn refs_under(&self, namespace: &str) -> Result<Vec<(String, String)>, UpstrokeError> {
        let output = self.git_ok(
            &self.base,
            &[
                OsString::from("for-each-ref"),
                OsString::from("--format=%(refname) %(objectname)"),
                OsString::from(namespace),
            ],
        )?;
        let listing = String::from_utf8(output).map_err(|error| UpstrokeError::Git {
            message: format!("`git for-each-ref {namespace}` returned non-UTF-8 output: {error}"),
        })?;
        Ok(listing
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(refname, oid)| (refname.to_owned(), oid.to_owned()))
            .collect())
    }

    /// Refuse a run namespace carrying anything `expected` does not name.
    ///
    /// `expected_failures_refusals[2]`: "unexpected refs under the run
    /// namespace".
    ///
    /// # Errors
    ///
    /// [`Refusal::UnexpectedRefUnderNamespace`] or a Git error.
    pub fn refuse_unexpected_refs(
        &self,
        namespace: &str,
        expected: &[String],
    ) -> Result<(), UpstrokeError> {
        for (refname, _) in self.refs_under(namespace)? {
            if !expected.contains(&refname) {
                return Err(Refusal::UnexpectedRefUnderNamespace {
                    namespace: namespace.to_owned(),
                    refname,
                }
                .into());
            }
        }
        Ok(())
    }

    /// Refuse a symbolic ref without touching it.
    fn refuse_symbolic(&self, refname: &str) -> Result<(), UpstrokeError> {
        let output = self.git(
            &self.base,
            &[
                OsString::from("symbolic-ref"),
                OsString::from("-q"),
                OsString::from("--"),
                OsString::from(refname),
            ],
        )?;
        if output.status.success() {
            return Err(Refusal::SymbolicRef {
                refname: refname.to_owned(),
                target: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            }
            .into());
        }
        Ok(())
    }

    fn update_ref(&self, args: &[&str]) -> Result<(), UpstrokeError> {
        let mut argv = vec![OsString::from("update-ref")];
        argv.extend(args.iter().map(OsString::from));
        self.git_ok(&self.base, &argv)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The Object group (R9 / R10 / R24 / R27)
    // -----------------------------------------------------------------------

    /// `Object.CandidateStage` — `git add -A` in the task worktree.
    ///
    /// The blob objects it writes are referenced by that worktree's index: R9,
    /// which is exactly what `ObjectSite::CandidateStage.row()` answers.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn candidate_stage(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
    ) -> Result<(), UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        funnel(
            hooks,
            EffectSiteId::Object(ObjectSite::CandidateStage),
            || {
                self.revalidate_chain(&path)?;
                self.git_ok(
                    &path,
                    &Self::CANDIDATE_STAGE_ARGV
                        .iter()
                        .map(OsString::from)
                        .collect::<Vec<_>>(),
                )?;
                Ok(())
            },
        )
    }

    /// `Object.CandidateWriteTree` — `git write-tree` in the task worktree.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn candidate_write_tree(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
    ) -> Result<String, UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        funnel(
            hooks,
            EffectSiteId::Object(ObjectSite::CandidateWriteTree),
            || {
                self.revalidate_chain(&path)?;
                self.git_line(&path, &Self::CANDIDATE_WRITE_TREE_ARGV)
            },
        )
    }

    /// `Object.SnapshotCommitTree` — the ephemeral commit of a tree-only
    /// snapshot input, on the recorded parent.
    ///
    /// Unreferenced when it is written (R27), and only `Snapshot.Add` moves it
    /// into R24.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn snapshot_commit_tree(
        &self,
        hooks: &mut dyn EffectHooks,
        tree: &str,
        parent: &str,
    ) -> Result<String, UpstrokeError> {
        self.commit_tree(
            hooks,
            EffectSiteId::Object(ObjectSite::SnapshotCommitTree),
            tree,
            parent,
            "upstroke: ephemeral snapshot input",
        )
    }

    /// `Object.CandidateCommitTree` — the candidate commit.
    ///
    /// Unreferenced when it is written (R27), and only
    /// `Ref.PinCandidatePrepared` moves it into R23.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn candidate_commit_tree(
        &self,
        hooks: &mut dyn EffectHooks,
        tree: &str,
        parent: &str,
        message: &str,
    ) -> Result<String, UpstrokeError> {
        self.commit_tree(
            hooks,
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
            tree,
            parent,
            message,
        )
    }

    /// The two commit-tree sites, including the parent-side `IdUnread` point
    /// they both expose.
    ///
    /// `effect_site_inventory.identity`: "the two commit-tree sites
    /// additionally expose the parent-side sub-effect point IdUnread (the child
    /// has exited with the object written; the coordinator has not yet read or
    /// recorded the printed id — R27 residue)".
    ///
    /// The point is consulted *after* `wait_with_output` and *before* the
    /// printed id is parsed. Buffering the child's stdout is not reading the
    /// id: the durable claim is that the coordinator has not **recorded** it,
    /// and a kill here leaves an object nothing names.
    fn commit_tree(
        &self,
        hooks: &mut dyn EffectHooks,
        site: EffectSiteId,
        tree: &str,
        parent: &str,
        message: &str,
    ) -> Result<String, UpstrokeError> {
        apply(
            hooks.phase(site, HookPhase::Before),
            site,
            HookPhase::Before,
        )?;
        let output = self.git_with_identity(
            &self.base,
            &[
                OsString::from("commit-tree"),
                OsString::from(tree),
                OsString::from("-p"),
                OsString::from(parent),
                OsString::from("-m"),
                OsString::from(message),
            ],
        )?;
        if !output.status.success() {
            return Err(UpstrokeError::Git {
                message: format!(
                    "`git commit-tree` failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        // The child has exited with the object written and the id is not yet
        // recorded. This is the whole of `IdUnread`.
        point(hooks, site, SubEffectPoint::IdUnread)?;
        let id = String::from_utf8(output.stdout)
            .map_err(|error| UpstrokeError::Git {
                message: format!("`git commit-tree` printed a non-UTF-8 id: {error}"),
            })?
            .trim()
            .to_owned();
        apply(hooks.phase(site, HookPhase::After), site, HookPhase::After)?;
        Ok(id)
    }

    /// `Object.ProposalCherryPick` — the proposal commit and its merge objects
    /// in the staging worktree of a stale candidate.
    ///
    /// Never executed for an exact-base fast sequence: `snapshots` and
    /// `resource_accounting[R10]` both say the staging worktree is "never
    /// created for an exact-base fast sequence", and the fast path's
    /// no-execution entry is asserted against that.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn proposal_cherry_pick(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        commit: &str,
    ) -> Result<String, UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        funnel(
            hooks,
            EffectSiteId::Object(ObjectSite::ProposalCherryPick),
            || {
                self.revalidate_chain(&path)?;
                let mut argv: Vec<OsString> = Self::PROPOSAL_CHERRY_PICK_ARGV
                    .iter()
                    .map(OsString::from)
                    .collect();
                argv.push(OsString::from(commit));
                self.git_ok(&path, &argv)?;
                self.git_line(&path, &["rev-parse", "HEAD"])
            },
        )
    }

    /// `Object.RepairMaterialize` — `git cherry-pick --no-commit` in a repair
    /// worktree.
    ///
    /// The merge objects it writes are referenced by that worktree's index: R9.
    /// `--no-commit` deliberately leaves `CHERRY_PICK_HEAD` behind, which is
    /// why the residue classifier reads the *index* for this site's after
    /// phase and never reads `CHERRY_PICK_HEAD` as residue on its own here.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn repair_materialize(
        &self,
        hooks: &mut dyn EffectHooks,
        slot: &Slot,
        commit: &str,
    ) -> Result<(), UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        funnel(
            hooks,
            EffectSiteId::Object(ObjectSite::RepairMaterialize),
            || {
                self.revalidate_chain(&path)?;
                self.git_ok(
                    &path,
                    &[
                        OsString::from("cherry-pick"),
                        OsString::from("--no-commit"),
                        OsString::from(commit),
                    ],
                )?;
                Ok(())
            },
        )
    }

    /// Whether `object` is an object this repository has.
    ///
    /// Read-only, and the read `T-DISPATCH`'s "source candidate object missing"
    /// refusal is made of: a repair is materialized from a *protected* candidate
    /// commit, and the alternative to asking is letting `git cherry-pick` fail
    /// with a message about a revision rather than about a lost candidate.
    ///
    /// `^{}` is the peel: it makes the question "is there an object here",
    /// following a tag to what it names, rather than "is there a ref by this
    /// spelling".
    ///
    /// # Errors
    ///
    /// A Git error other than "no such object", which is the `false` answer.
    pub fn object_exists(&self, object: &str) -> Result<bool, UpstrokeError> {
        object_exists(&self.base, object)
    }

    // -----------------------------------------------------------------------
    // Byte-safe changed paths
    // -----------------------------------------------------------------------

    /// The paths a worktree's index changed against `base`, byte-safely.
    ///
    /// `topology::paths::PathSet::RepoWide` is "the classification for an
    /// absent, unsafe, unparsable, or **undecodable** answer", and
    /// `GitPath`'s own documentation says "paths that did not decode are never
    /// stored". So the capture reads `-z` bytes, never lines, and one
    /// undecodable path makes the whole answer repo-wide rather than a silently
    /// shorter list.
    ///
    /// # Why `--name-status -M` and not `--name-only`
    ///
    /// `decisions.admission_and_leases.path_policy.actual` is "`git diff-tree
    /// -r -z -M --name-status base tree`; **both rename endpoints**", and
    /// `--name-only` cannot satisfy that sentence. Rename detection is Git's
    /// **default** (`diff.renames` has been true since 2.9), and a detected
    /// rename under `--name-only` prints the destination alone — measured on
    /// git 2.43, where staging `src/auth.rs -> archive/auth.rs` printed
    /// `archive/auth.rs` and nothing else. The old endpoint is the one another
    /// owner may hold a lease on, so dropping it lets two overlapping edits be
    /// admitted at once, which is exactly what `overlap` exists to prevent
    /// (`PR5-CORRECTNESS-005`).
    ///
    /// `-M` is passed explicitly rather than left to configuration, so the
    /// records do not depend on the operator's `diff.renames`, and the status
    /// field is what tells a two-endpoint record from a one-endpoint one.
    ///
    /// `git diff --cached <base>` rather than the passage's `diff-tree base
    /// tree`: this primitive is asked what a worktree's *index* holds, which is
    /// the tree that has not been written yet. The two produce byte-identical
    /// `-z --name-status` records for the same content — measured — and `-r` is
    /// a `diff-tree` option only, because `git diff` always recurses.
    ///
    /// # Errors
    ///
    /// The containment refusals or a Git error.
    pub fn changed_paths(&self, slot: &Slot, base: &str) -> Result<PathSet, UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        let output = self.git_ok(
            &path,
            &[
                OsString::from("diff"),
                OsString::from("--cached"),
                OsString::from("--name-status"),
                OsString::from("-M"),
                OsString::from("-z"),
                OsString::from(base),
            ],
        )?;
        Ok(decode_changed_paths(&output))
    }

    /// The diff of a captured candidate tree against the commit it is judged
    /// against.
    ///
    /// **A read, so it takes no hooks and names no effect site.** It creates no
    /// object, moves no ref and touches no worktree — the same reason
    /// [`Self::changed_paths`] is not a funnel. The frozen `ObjectSite` enum
    /// has no diff variant, and it should not: every variant there documents
    /// "the row that references the created object immediately after the
    /// effect", and a diff creates nothing to reference.
    ///
    /// The flags come from [`crate::workspace::REVIEW_DIFF_FLAGS`], shared with
    /// the schema-3 capture, because both produce the text a reviewer judges
    /// and `classify::diff_failure` reads. Two flag lists would be two
    /// definitions of what a reviewable diff is.
    ///
    /// Run from the task worktree so the object names resolve in the repository
    /// that holds them.
    ///
    /// # Errors
    ///
    /// The containment refusals, a Git error, or a diff whose bytes are not
    /// UTF-8 — which is not a diff any reviewer can be shown.
    pub fn candidate_diff(
        &self,
        slot: &Slot,
        parent: &str,
        tree: &str,
    ) -> Result<String, UpstrokeError> {
        self.revalidate()?;
        let path = self.slot_target(slot)?;
        let mut argv: Vec<OsString> = crate::workspace::REVIEW_DIFF_FLAGS
            .iter()
            .map(OsString::from)
            .collect();
        argv.extend([
            OsString::from(parent),
            OsString::from(tree),
            OsString::from("--"),
        ]);
        let output = self.git_ok(&path, &argv)?;
        String::from_utf8(output).map_err(|_| UpstrokeError::Git {
            message: format!(
                "the diff of {tree} against {parent} is not valid UTF-8; a reviewer cannot be \
                 shown it and a gate would not agree with what it says"
            ),
        })
    }

    /// A commit's first parent, or `None` when the object is not a commit.
    ///
    /// **A read**, like its neighbours. `rev-parse <sha>^` answers with the
    /// parent and errors for a blob or a tree, so "not a commit" and "a commit
    /// with no parent" both arrive here as `None` — which is the same answer
    /// for the caller's purpose: neither is a candidate on a recorded base.
    ///
    /// # Errors
    ///
    /// The containment refusals.
    pub fn commit_parent(&self, commit: &str) -> Result<Option<String>, UpstrokeError> {
        self.revalidate()?;
        let argv = [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(format!("{commit}^{{commit}}^")),
        ];
        Ok(self
            .git_ok(self.base(), &argv)
            .ok()
            .and_then(|out| String::from_utf8(out).ok())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()))
    }

    /// The tree a commit points at, or `None` if it is not a commit.
    ///
    /// The sibling of [`Self::commit_parent`] and deliberately the same shape:
    /// `rev-parse --verify --quiet` with a peel, so a missing object, a
    /// non-commit and a malformed id all arrive as `None` rather than as three
    /// different errors the caller would have to tell apart. What the caller
    /// does with `None` is refuse, and it refuses the same way for all three.
    ///
    /// Added for candidate adoption: `DESIGN.md` §15 requires resume to adopt
    /// only the exact judged object, and the parent alone does not say what the
    /// commit *contains*.
    ///
    /// # Errors
    ///
    /// The containment refusals.
    pub fn commit_tree_sha(&self, commit: &str) -> Result<Option<String>, UpstrokeError> {
        self.revalidate()?;
        let argv = [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--quiet"),
            OsString::from(format!("{commit}^{{commit}}^{{tree}}")),
        ];
        Ok(self
            .git_ok(self.base(), &argv)
            .ok()
            .and_then(|out| String::from_utf8(out).ok())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()))
    }

    // -----------------------------------------------------------------------
    // Git plumbing
    // -----------------------------------------------------------------------

    /// The hooks path, walked immediately before every Git command.
    ///
    /// Every command runs with `core.hooksPath` at [`Self::hooks_dir`], and a
    /// hook that ran from there would be an effect no site accounts for. The
    /// in-funnel chain check walks the effect's own target and not this path,
    /// so a `hooks-none` exchanged for a link to a directory holding an
    /// executable `post-checkout` after that check would have Git execute it.
    /// So the chain from the private root down to `hooks-none` is walked
    /// here, adjacent to the spawn, for every command: each component a real
    /// directory and no reparse point. Absence is allowed — a root not yet
    /// created has no hooks directory, and Git runs no hook from a path that
    /// does not exist — and the same window between check and spawn remains
    /// that [`Self::revalidate_chain`] describes.
    ///
    /// # Errors
    ///
    /// [`Refusal::BaseIsNotADirectory`], [`Refusal::ReparsePointOnChain`], or
    /// an I/O error naming the component that could not be read.
    fn revalidate_hooks_path(&self) -> Result<(), UpstrokeError> {
        refuse_reparse_points(&self.private_root, &self.hooks_dir())
    }

    /// Run Git in `cwd` with every repository hook and the fsmonitor disabled.
    ///
    /// The hooks path is walked first ([`Self::revalidate_hooks_path`]).
    fn git(&self, cwd: &Path, args: &[OsString]) -> Result<Output, UpstrokeError> {
        self.revalidate_hooks_path()?;
        self.command(cwd, args)
            .output()
            .map_err(|error| UpstrokeError::Git {
                message: format!("failed to run git: {error}"),
            })
    }

    fn command(&self, cwd: &Path, args: &[OsString]) -> Command {
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(self.hooks_dir());
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(cwd)
            .arg("-c")
            .arg(hooks_config)
            .args(["-c", "core.fsmonitor=false"])
            .args(["-c", "protocol.file.allow=never"])
            .args(args)
            .stdin(Stdio::null());
        command
    }

    fn git_with_identity(&self, cwd: &Path, args: &[OsString]) -> Result<Output, UpstrokeError> {
        self.revalidate_hooks_path()?;
        self.command(cwd, args)
            // Environment identity overrides repository and global config and
            // any inherited GIT_AUTHOR_*/GIT_COMMITTER_*, so a commit-tree is a
            // function of its inputs and not of the machine.
            .env("GIT_AUTHOR_NAME", "upstroke")
            .env("GIT_AUTHOR_EMAIL", "upstroke@upstroke.local")
            .env("GIT_AUTHOR_DATE", "@0 +0000")
            .env("GIT_COMMITTER_NAME", "upstroke")
            .env("GIT_COMMITTER_EMAIL", "upstroke@upstroke.local")
            .env("GIT_COMMITTER_DATE", "@0 +0000")
            .output()
            .map_err(|error| UpstrokeError::Git {
                message: format!("failed to run git: {error}"),
            })
    }

    fn git_ok(&self, cwd: &Path, args: &[OsString]) -> Result<Vec<u8>, UpstrokeError> {
        let output = self.git(cwd, args)?;
        if !output.status.success() {
            return Err(UpstrokeError::Git {
                message: format!(
                    "git {} failed in {}: {}",
                    args.iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join(" "),
                    cwd.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(output.stdout)
    }

    fn git_line(&self, cwd: &Path, args: &[&str]) -> Result<String, UpstrokeError> {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let output = self.git_ok(cwd, &argv)?;
        let text = String::from_utf8(output).map_err(|error| UpstrokeError::Git {
            message: format!("git {} returned non-UTF-8 output: {error}", args.join(" ")),
        })?;
        Ok(text.trim().to_owned())
    }

    /// Every registered worktree of the managed repository.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn worktree_records(&self) -> Result<Vec<WorktreeRecord>, UpstrokeError> {
        let output = self.git_ok(
            &self.base,
            &[
                OsString::from("worktree"),
                OsString::from("list"),
                OsString::from("--porcelain"),
                OsString::from("-z"),
            ],
        )?;
        Ok(parse_worktree_records(&output))
    }

    fn worktree_record(&self, path: &Path) -> Result<Option<WorktreeRecord>, UpstrokeError> {
        let wanted = canonical_prefix(path)?;
        for record in self.worktree_records()? {
            if canonical_prefix(&record.path)? == wanted {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    /// The per-worktree administrative directory of a linked worktree.
    fn worktree_git_dir(&self, path: &Path) -> Result<Option<PathBuf>, UpstrokeError> {
        let pointer = path.join(".git");
        let text = match fs::read_to_string(&pointer) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: pointer,
                    source,
                });
            }
        };
        let Some(target) = text.trim().strip_prefix("gitdir:") else {
            return Ok(None);
        };
        Ok(Some(PathBuf::from(target.trim())))
    }

    /// Revalidate containment without asking Git to parse a registration that
    /// recovery exists to remove.
    ///
    /// A zero-length `commondir` makes `git worktree list` fail before it emits
    /// any records. The registration's `gitdir` is still sufficient evidence
    /// when read byte-for-byte: it names the checkout's `.git`, whose parent
    /// must canonical-prefix to the exact slot target. Any unreadable, empty or
    /// partial `gitdir` refuses; guessing from the admin directory's basename
    /// would authorize deletion from a Git-generated, collision-suffixed name.
    fn revalidate_removal(&self, target: &Path) -> Result<Option<PathBuf>, UpstrokeError> {
        self.revalidate_chain(&self.execution_root)?;
        let root = canonical_prefix(&self.execution_root)?;
        let target = canonical_prefix(target)?;
        let base = canonical_prefix(&self.base)?;
        if is_at_or_inside(&base, &root) {
            return Err(Refusal::RootInsideRepositoryWorktree {
                root,
                worktree: self.base.clone(),
            }
            .into());
        }
        if is_at_or_inside(&root, &base) {
            return Err(Refusal::WorktreeInsideRoot {
                root,
                worktree: self.base.clone(),
            }
            .into());
        }

        let worktrees = self.common_git_dir.join("worktrees");
        let entries = match fs::read_dir(&worktrees) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // No registrations at all. Nothing to remove only if the
                // target is absent too; a target that is there with no
                // registration directory is the I/O failure it looks like,
                // and a target that cannot be read is its own.
                return match fs::symlink_metadata(&target) {
                    Err(absent) if absent.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Ok(_) => Err(UpstrokeError::Io {
                        path: worktrees,
                        source: error,
                    }),
                    Err(source) => Err(UpstrokeError::Io {
                        path: target,
                        source,
                    }),
                };
            }
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: worktrees,
                    source,
                });
            }
        };
        let mut matched = None;
        for entry in entries {
            let entry = entry.map_err(|source| UpstrokeError::Io {
                path: worktrees.clone(),
                source,
            })?;
            let admin = entry.path();
            let gitdir = admin.join("gitdir");
            let bytes = match fs::read(&gitdir) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(UpstrokeError::Io {
                        path: gitdir,
                        source,
                    });
                }
            };
            let checkout = registration_checkout(&admin, &bytes)?;
            let worktree = canonical_prefix(&checkout)?;
            if is_at_or_inside(&worktree, &root) {
                return Err(Refusal::RootInsideRepositoryWorktree {
                    root,
                    worktree: checkout.clone(),
                }
                .into());
            }
            if is_at_or_inside(&root, &worktree) && !self.is_manager_slot_path(&root, &worktree) {
                return Err(Refusal::WorktreeInsideRoot {
                    root,
                    worktree: checkout.clone(),
                }
                .into());
            }
            if worktree != target {
                continue;
            }
            if matched.replace(admin).is_some() {
                return Err(UpstrokeError::Git {
                    message: format!(
                        "more than one worktree registration names {}",
                        checkout.display()
                    ),
                });
            }
        }
        Ok(matched)
    }

    /// Re-read the registration identity at the destructive administration
    /// boundary. `false` is convergence, not permission to select the admin by
    /// another property: the `gitdir` or its directory is already gone.
    fn registration_still_names(&self, admin: &Path, target: &Path) -> Result<bool, UpstrokeError> {
        let gitdir = admin.join("gitdir");
        let bytes = match fs::read(&gitdir) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: gitdir,
                    source,
                });
            }
        };
        let checkout = registration_checkout(admin, &bytes)?;
        if canonical_prefix(&checkout)? != canonical_prefix(target)? {
            return Err(UpstrokeError::Git {
                message: format!(
                    "worktree registration {} changed identity before removal",
                    admin.display()
                ),
            });
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Git output decoders
// ---------------------------------------------------------------------------

mod parsers;
pub use self::parsers::decode_changed_paths;
use self::parsers::{parse_worktree_records, registration_checkout};

mod snapshot_ref;
pub use self::snapshot_ref::{Snapshot, SnapshotInput};

// ---------------------------------------------------------------------------
// Residue classification
// ---------------------------------------------------------------------------

mod residue;
use self::residue::administrative_residue_at;
pub use self::residue::{
    ResidueTarget, classify_object_residue, element_breaks_quiescence, observed_residue_elements,
    residue_classified_sites,
};

fn git_dir_of(worktree: &Path) -> Result<Option<PathBuf>, UpstrokeError> {
    let pointer = worktree.join(".git");
    match fs::metadata(&pointer) {
        Ok(metadata) if metadata.is_dir() => return Ok(Some(pointer)),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(UpstrokeError::Io {
                path: pointer,
                source,
            });
        }
    }
    let text = fs::read_to_string(&pointer).map_err(|source| UpstrokeError::Io {
        path: pointer.clone(),
        source,
    })?;
    Ok(text
        .trim()
        .strip_prefix("gitdir:")
        .map(|target| PathBuf::from(target.trim())))
}

fn read_only_git(cwd: &Path, args: &[&str]) -> Result<Output, UpstrokeError> {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| UpstrokeError::Git {
            message: format!("failed to run git: {error}"),
        })
}

fn read_only_git_ok(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, UpstrokeError> {
    let output = read_only_git(cwd, args)?;
    if !output.status.success() {
        return Err(UpstrokeError::Git {
            message: format!(
                "git {} failed in {}: {}",
                args.join(" "),
                cwd.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(output.stdout)
}

/// Every object `git fsck --unreachable` reports, and nothing else.
///
/// # Errors
///
/// A Git error.
pub fn unreachable_objects(worktree: &Path) -> Result<Vec<String>, UpstrokeError> {
    let output = read_only_git(
        worktree,
        &[
            "fsck",
            "--unreachable",
            "--no-progress",
            "--no-dangling",
            "--connectivity-only",
        ],
    )?;
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing
        .lines()
        .filter_map(|line| line.strip_prefix("unreachable "))
        .filter_map(|rest| rest.split_whitespace().nth(1))
        .map(std::borrow::ToOwned::to_owned)
        .collect())
}

/// Whether Git's own temporary object files are present.
///
/// Git writes a loose object to `objects/tmp_obj_XXXXXX` and renames it into
/// place, and packs to `objects/pack/tmp_pack_*`. `resource_accounting[R27]`
/// accounts for both and says "Git prunes temporary object files itself".
///
/// # Errors
///
/// A Git or I/O error.
pub fn temporary_object_files(worktree: &Path) -> Result<bool, UpstrokeError> {
    let object_dir = object_directory(worktree)?;
    for (directory, prefix) in [
        (object_dir.clone(), "tmp_obj_"),
        (object_dir.join("pack"), "tmp_pack_"),
    ] {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: directory,
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| UpstrokeError::Io {
                path: directory.clone(),
                source,
            })?;
            if entry.file_name().to_string_lossy().starts_with(prefix) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The repository's object directory.
///
/// # Errors
///
/// A Git error.
pub fn object_directory(worktree: &Path) -> Result<PathBuf, UpstrokeError> {
    let output = read_only_git_ok(
        worktree,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "objects",
        ],
    )?;
    let text = String::from_utf8(output).map_err(|error| UpstrokeError::Git {
        message: format!("`git rev-parse --git-path objects` returned non-UTF-8: {error}"),
    })?;
    Ok(PathBuf::from(text.trim()))
}

fn object_exists(worktree: &Path, object: &str) -> Result<bool, UpstrokeError> {
    let output = read_only_git(worktree, &["cat-file", "-e", &format!("{object}^{{}}")])?;
    Ok(output.status.success())
}

fn head_commit(worktree: &Path) -> Result<Option<String>, UpstrokeError> {
    let output = read_only_git(worktree, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn index_lock_present(worktree: &Path) -> Result<bool, UpstrokeError> {
    Ok(git_dir_of(worktree)?.is_some_and(|dir| dir.join("index.lock").exists()))
}

/// Whether anything in the working tree is not yet in the index.
fn worktree_has_unstaged_changes(worktree: &Path) -> Result<bool, UpstrokeError> {
    // `--no-renames` is load-bearing, not tidiness: `status --porcelain -z`
    // detects renames by default and then emits `R  <new>\0<old>\0`, so the
    // *old path* arrives as a bare field whose second byte is a path character
    // and would be read as an unstaged status.
    let output = read_only_git_ok(
        worktree,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
    )?;
    for entry in output.split(|byte| *byte == 0) {
        if entry.len() < 2 {
            continue;
        }
        let worktree_status = entry[1];
        if worktree_status != b' ' {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the index has anything staged against HEAD.
fn index_differs_from_head(worktree: &Path) -> Result<bool, UpstrokeError> {
    let output = read_only_git(worktree, &["diff", "--cached", "--quiet"])?;
    Ok(!output.status.success())
}

/// The registration `repository` holds for `worktree`, if any.
///
/// The question is asked of the **repository**, never of the worktree: a killed
/// `git worktree add` can leave a registration whose checkout directory does not
/// exist, and asking a directory that is not there — or asking its parent, which
/// is inside the execution root and is not a repository at all — would answer
/// "nothing is registered" for exactly the residue this is here to see.
fn record_for(repository: &Path, worktree: &Path) -> Result<Option<WorktreeRecord>, UpstrokeError> {
    let output = read_only_git(repository, &["worktree", "list", "--porcelain", "-z"])?;
    if !output.status.success() {
        return Ok(None);
    }
    let wanted = canonical_prefix(worktree)?;
    for record in parse_worktree_records(&output.stdout) {
        if canonical_prefix(&record.path)? == wanted {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Object ids and the ref-transition refusals
// ---------------------------------------------------------------------------

mod object;
pub use self::object::{is_null_object_id, is_object_id};
use self::object::{refuse_expected_old, refuse_malformed_object_id};

// ---------------------------------------------------------------------------
// Small filesystem helpers
// ---------------------------------------------------------------------------

/// Write `bytes` durably: temporary, fsync, rename, fsync the directory.
///
/// Every one of those four steps that is a *durability* step records itself in
/// `ledger`, fused with the primitive it records — the sync and its entry are
/// one call, so a mutation that removes a step from this sequence removes its
/// evidence with it. The residual boundary is the same one the Event lane
/// states in writing: deleting the `sync_all` line *inside* the fused helper is
/// still undetectable by any test on a machine that does not lose power.
fn write_synced(path: &Path, bytes: &[u8], ledger: &DurabilityLedger) -> Result<(), UpstrokeError> {
    let parent = path.parent().ok_or_else(|| UpstrokeError::Git {
        message: format!("{} has no parent directory", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(|source| UpstrokeError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let name = path.file_name().ok_or_else(|| UpstrokeError::Git {
        message: format!("{} has no file name", path.display()),
    })?;
    // A per-call unique staging name, and `create_new`: a fixed name is a
    // name anyone can plant, and `File::create` follows a link planted there
    // to whatever it names. `create_new` refuses an existing name of any
    // kind, link included, so the staged file is this call's alone.
    let mut staged_name = OsString::from(".");
    staged_name.push(name);
    staged_name.push(format!(".{}.tmp", crate::ulid::ulid()));
    let staged = parent.join(staged_name);
    let written = {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|source| UpstrokeError::Io {
                path: staged.clone(),
                source,
            })?;
        file.write_all(bytes)
            .map_err(|source| UpstrokeError::Io {
                path: staged.clone(),
                source,
            })
            .and_then(|()| sync_file_recorded(&file, &staged, ledger))
    };
    let landed = written.and_then(|()| {
        fs::rename(&staged, path).map_err(|source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        })
    });
    if let Err(error) = landed {
        // The staged file is ours alone, so a refused attempt leaves nothing
        // behind — or names what it left.
        return Err(match fs::remove_file(&staged) {
            Ok(()) => error,
            Err(gone) if gone.kind() == std::io::ErrorKind::NotFound => error,
            Err(cleanup) => UpstrokeError::Io {
                path: staged,
                source: std::io::Error::new(
                    cleanup.kind(),
                    format!("{error}; and the staged file could not be removed: {cleanup}"),
                ),
            },
        });
    }
    let length = fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    ledger.record(DurableStep::Renamed, path, length);
    sync_directory(parent, ledger)
}

/// fsync `file` and record what was made durable, in one call.
fn sync_file_recorded(
    file: &fs::File,
    path: &Path,
    ledger: &DurabilityLedger,
) -> Result<(), UpstrokeError> {
    let io = |source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let outcome = crate::util::fsync_file(file);
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    ledger.record(DurableStep::SyncedFile, path, len);
    outcome.map_err(io)
}

/// fsync a directory, on every platform, and record it (`PR5-CONF-013`).
///
/// The barrier itself is [`crate::util::fsync_dir`], shared with the run-directory
/// and Event funnels so that the one Win32 recipe there is is written once.
fn sync_directory(path: &Path, ledger: &DurabilityLedger) -> Result<(), UpstrokeError> {
    crate::util::fsync_dir(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    ledger.record(DurableStep::SyncedDirectory, path, 0);
    Ok(())
}

fn directory_is_empty(path: &Path) -> Result<bool, UpstrokeError> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(source) => Err(UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// The repository's canonical common git dir.
fn common_git_dir(inside: &Path) -> Result<PathBuf, UpstrokeError> {
    let output = read_only_git_ok(
        inside,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let text = String::from_utf8(output).map_err(|error| UpstrokeError::Git {
        message: format!("`git rev-parse --git-common-dir` returned non-UTF-8: {error}"),
    })?;
    let path = PathBuf::from(text.trim());
    fs::canonicalize(&path)
        .map(strip_verbatim)
        .map_err(|source| UpstrokeError::Io { path, source })
}

/// The git, worktree and process effects a **test in another module** needs.
///
/// `src/engine/topology/**` is a topology module: `clippy.toml` denies
/// `std::fs::write`, `std::fs::create_dir_all`, `std::process::Command` and
/// their neighbours there, and the denial applies to `#[cfg(test)]` code as
/// well — measured, four errors from a probe module that did nothing but call
/// them. A schema-4 test still has to build a real repository, put bytes in a
/// worktree, and spawn a child it can kill, and no funnel owns `git init`.
///
/// So the primitives live here, in the funnel module `effects/allowlist.toml`
/// already reviews, and every one of them is `#[cfg(test)]`. Since W1 this
/// module is a **sibling file** rather than a block nested in this one, so it
/// states a lint level **of its own** — `src/workspace_manager/fixture.rs`
/// allows `disallowed_methods` and `disallowed_types`, a subset of this file's
/// three, and re-denies `disallowed_macros` — and carries its own
/// `effects/allowlist.toml` row. Inheriting this file's allow through the
/// module tree is `PR6-LANEF-004`, and that prologue exists to refuse it.
///
/// [`Fixture`] and the three helpers above it were `mod tests`'s and are
/// **moved** rather than copied. A second repository fixture maintained beside
/// this one is the class this crate has already recorded three times: two
/// hand-maintained copies of one value disagree eventually, and the copy that
/// disagrees silently is the one a census stands on.
#[cfg(test)]
pub(crate) mod fixture;

#[cfg(test)]
mod tests;
