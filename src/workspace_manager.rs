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
//! which answers [`Injection::Proceed`] and records nothing; the ST-07 subset
//! passes [`HarnessEffects`], which records into PR3's [`HookHarness`].
//!
//! The after hook is **not** called when the primitive returned `Err`. The
//! after phase's claim is `AfterEffect::Referenced` / `Unreferenced` /
//! `Released` — "the artifact is present and referenced by the row `row()`
//! names" — and a funnel that ran it after a failed primitive would record an
//! execution of a phase whose claim is false, which is the same false report
//! [`HookHarness`] exists to prevent.
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

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::error::UpstrokeError;
use crate::topology::effects::{
    EffectSiteId, HookHarness, HookPhase, Injection, InjectionMode, ObjectResidue, ObjectSite,
    RefSite, ResidueElement, ResourceRow, SnapshotSite, SubEffectPoint, WorktreeSite,
};
use crate::topology::paths::{GitPath, PathSet};
use crate::util::{DurabilityLedger, DurableStep};

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// What a funnel tells whoever is watching, at both hook phases and at the
/// parent-side sub-effect points.
///
/// The shape mirrors [`crate::agent::proc::SpawnHooks`], which PR4 wired onto
/// the same [`HookHarness`], except that these funnels serve many sites each,
/// so the site travels with the call.
pub trait EffectHooks {
    /// The funnel reached `phase` of `site`. The answer says what it must do.
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection;

    /// Where this observer wants the funnel's durability primitives recorded.
    ///
    /// A *handle*, taken before the funnel body runs, rather than a method the
    /// body calls back into: `funnel` already holds `&mut dyn EffectHooks` for
    /// the whole call, so a body that also needed the observer would be a
    /// second mutable borrow of it. The handle is cloneable and shares its log,
    /// so what the body records is what the caller reads.
    ///
    /// The default records nothing, which is what production passes and what
    /// every observer that does not care about durability inherits.
    fn durability_ledger(&self) -> DurabilityLedger {
        DurabilityLedger::off()
    }
}

/// What production passes: nothing is armed and nothing is recorded.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl EffectHooks for NoHooks {
    fn phase(&mut self, _site: EffectSiteId, _phase: HookPhase) -> Injection {
        Injection::Proceed
    }
}

/// Wires these funnels onto PR3's [`HookHarness`], the way
/// [`crate::runner::HarnessHooks`] wires the process funnel onto it.
#[derive(Debug, Clone, Default)]
pub struct HarnessEffects {
    harness: Arc<Mutex<HookHarness>>,
    ledger: DurabilityLedger,
}

impl HarnessEffects {
    /// Observe through `harness`.
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self {
            harness,
            ledger: DurabilityLedger::off(),
        }
    }

    /// The harness this observer records into.
    #[must_use]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }

    /// Also record every durability primitive the funnels perform.
    #[must_use]
    pub fn recording_durability(mut self) -> Self {
        self.ledger = DurabilityLedger::recording();
        self
    }

    /// The durability ledger this observer records into.
    #[must_use]
    pub fn ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }
}

impl EffectHooks for HarnessEffects {
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        let mut harness = self
            .harness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        harness.hook(site, phase)
    }

    fn durability_ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }
}

/// Do what a hook answered.
///
/// [`Injection::Kill`] aborts, for the reason
/// [`crate::agent::proc`] already gives: the claim under test is what a
/// coordinator that dies **without running any cleanup** leaves durable, and
/// both `panic!` and `std::process::exit` run destructors.
fn apply(injection: Injection, site: EffectSiteId, phase: HookPhase) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!("the `{site}` funnel was made to fail at its `{phase}` phase"),
        }),
    }
}

/// `hook(Before, site) -> primitive -> hook(After, site)`, once.
fn funnel<T, F>(
    hooks: &mut dyn EffectHooks,
    site: EffectSiteId,
    primitive: F,
) -> Result<T, UpstrokeError>
where
    F: FnOnce() -> Result<T, UpstrokeError>,
{
    apply(
        hooks.phase(site, HookPhase::Before),
        site,
        HookPhase::Before,
    )?;
    let value = primitive()?;
    apply(hooks.phase(site, HookPhase::After), site, HookPhase::After)?;
    Ok(value)
}

/// Consult a parent-side sub-effect point, in every mode the point declares.
///
/// The harness is keyed by `(site, point, mode)` because "a mode is executed
/// when its fault fired", so one funnel position consults it once per declared
/// mode and the first non-`Proceed` answer wins. [`SubEffectPoint::IdUnread`]
/// declares `Kill` alone, so in practice this is one call — but the loop is
/// over [`SubEffectPoint::modes`] rather than over a literal, so a point that
/// gains a mode is consulted for it.
fn point(
    hooks: &mut dyn EffectHooks,
    site: EffectSiteId,
    at: SubEffectPoint,
) -> Result<(), UpstrokeError> {
    let mut decision = Injection::Proceed;
    for mode in at.modes() {
        let answer = hooks.phase(
            site,
            HookPhase::Point {
                point: at,
                mode: *mode,
            },
        );
        if decision == Injection::Proceed {
            decision = answer;
        }
    }
    apply(
        decision,
        site,
        HookPhase::Point {
            point: at,
            mode: InjectionMode::Kill,
        },
    )
}

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
        "refusing {}: `{}` on the chain is a symlink or reparse point, and \
         decisions.workspace_candidates.execution_root creates a root only when the chain carries \
         none",
        .chain.display(),
        .at.display()
    )]
    ReparsePointOnChain {
        /// The path whose chain was walked.
        chain: PathBuf,
        /// The component that is a symlink, junction, or other reparse point.
        at: PathBuf,
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
#[must_use]
pub fn execution_root_of(private_root: &Path, repo_key: &str, run_id: &str) -> PathBuf {
    private_root.join("workspaces").join(repo_key).join(run_id)
}

/// Whether `metadata` describes a symlink, junction, or any other reparse
/// point.
///
/// **Windows and Unix answer different questions on purpose.** On Unix the only
/// such object is a symbolic link. On Windows the set is larger — a directory
/// junction (`mklink /J`) and a mount point are reparse points that are *not*
/// symbolic links, and `FileType::is_symlink` answers true only for the
/// name-surrogate tags. `expected_failures_refusals[0]` is "symlink/**junction**
/// on the chain", so the Windows half reads the raw attribute
/// (`FILE_ATTRIBUTE_REPARSE_POINT`) instead, which is true for every reparse
/// point whatever its tag. A refusal that fired only on POSIX symlinks would
/// pass every Linux test and refuse nothing a Windows operator can build.
#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// See the Windows half for why the two differ.
#[cfg(not(windows))]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// The first component of `path`'s chain **at or below `anchor`** that is a
/// reparse point, if any.
///
/// # Why the walk is anchored
///
/// `decisions.workspace_candidates.execution_root` says "with no
/// symlink/reparse point on the chain", and a chain has to start somewhere.
/// It starts at the operator's own authorized root, canonicalized — which is
/// how the packet anchors the same check on the other half of the same
/// structure: `expected_failures_refusals[9]` requires "a locator chain without
/// reparse points **canonicalizing to** `<authorized private root>/runs/
/// <basename>`". The root is resolved and trusted; what must be reparse-free is
/// everything the run itself builds beneath it.
///
/// The unanchored reading was tried and is wrong on a real platform, not just
/// inconvenient: macOS ships `/var` as a symlink to `private/var` and its
/// `$TMPDIR` lives under it, so an operator whose private root is anywhere
/// under `/var` — including every default temporary directory on that OS —
/// would have every run refused for a link they did not create and cannot
/// remove. No live passage asks for that, and the containment the refusal
/// exists to protect is unaffected: every deletion in this module goes through
/// [`WorkspaceManager::contained`], which compares **canonical** paths, so a
/// resolved link cannot carry a removal outside the root.
///
/// Only components that exist are inspected: a root that has not been created
/// yet has an absent leaf, and refusing on absence would refuse every first
/// run.
fn reparse_point_below(anchor: &Path, path: &Path) -> Result<Option<PathBuf>, UpstrokeError> {
    let Ok(relative) = path.strip_prefix(anchor) else {
        return Ok(None);
    };
    let mut walked = anchor.to_path_buf();
    for component in relative.components() {
        walked.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match fs::symlink_metadata(&walked) {
            Ok(metadata) => {
                if is_reparse_point(&metadata) {
                    return Ok(Some(walked));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(UpstrokeError::Io {
                    path: walked,
                    source,
                });
            }
        }
    }
    Ok(None)
}

/// Refuse `path` when a component of its chain below `anchor` is a reparse
/// point.
fn refuse_reparse_points(anchor: &Path, path: &Path) -> Result<(), UpstrokeError> {
    if let Some(at) = reparse_point_below(anchor, path)? {
        return Err(Refusal::ReparsePointOnChain {
            chain: path.to_path_buf(),
            at,
        }
        .into());
    }
    Ok(())
}

/// The leaf clause of `execution_root`: "the managed base is a **real
/// directory**".
fn refuse_unreal_directory(path: &Path) -> Result<(), UpstrokeError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(Refusal::BaseIsNotADirectory {
            path: path.to_path_buf(),
        }
        .into());
    }
    Ok(())
}

/// Undo Windows' extended-length (`\\?\`) canonical form.
///
/// **Measured on the Windows Server 2025 guest**, and a production defect
/// rather than a test artefact: `fs::canonicalize` on Windows returns
/// `\\?\C:\...`, and Git — an MSYS program — rewrites that to `//?/C:/...`
/// and fails with `could not create leading directories … Invalid argument`.
/// Every `git worktree add` under an execution root derived from a
/// canonicalized private root failed with it. Whatever this module hands to Git
/// has to be a path Git can open, so the verbatim prefix comes back off.
///
/// A path that genuinely *requires* the verbatim form — one longer than
/// `MAX_PATH`, or carrying a component Win32 would reject — is left as it is:
/// stripping it would produce a path that names something else, and Git could
/// not have used either spelling.
#[cfg(windows)]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    use std::path::Prefix;

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut rebuilt = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:\\", letter as char)),
        Prefix::VerbatimUNC(server, share) => {
            let mut unc = PathBuf::from("\\\\");
            unc.push(server);
            unc.push(share);
            unc
        }
        _ => return path,
    };
    for component in components {
        if matches!(component, Component::RootDir) {
            continue;
        }
        rebuilt.push(component.as_os_str());
    }
    rebuilt
}

/// See the Windows half: nothing to undo anywhere else.
#[cfg(not(windows))]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Canonicalize the longest existing prefix of `path` and rejoin the rest.
///
/// `fs::canonicalize` needs the whole path to exist; an execution root is
/// compared for containment before it does.
fn canonical_prefix(path: &Path) -> Result<PathBuf, UpstrokeError> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(strip_verbatim(canonical));
    }
    let mut tail = Vec::new();
    let mut head = path.to_path_buf();
    loop {
        let Some(parent) = head.parent().map(Path::to_path_buf) else {
            return Ok(path.to_path_buf());
        };
        let Some(name) = head.file_name().map(OsStr::to_os_string) else {
            return Ok(path.to_path_buf());
        };
        tail.push(name);
        head = parent;
        if let Ok(canonical) = fs::canonicalize(&head) {
            let mut canonical = strip_verbatim(canonical);
            for name in tail.iter().rev() {
                canonical.push(name);
            }
            return Ok(canonical);
        }
        if head.parent().is_none() {
            return Ok(path.to_path_buf());
        }
    }
}

/// Whether `inner` is `outer` or lies beneath it. Both must already be
/// canonical-prefixed.
fn is_at_or_inside(outer: &Path, inner: &Path) -> bool {
    inner == outer || inner.starts_with(outer)
}

// ---------------------------------------------------------------------------
// Slots: the worktree, staging, and snapshot names the packet gives
// ---------------------------------------------------------------------------

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
fn safe_component(name: &str) -> Option<&'static str> {
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
    fn validate(&self) -> Result<(), Refusal> {
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
    fn from_intent_name(name: &str) -> Option<Self> {
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

// ---------------------------------------------------------------------------
// The manager
// ---------------------------------------------------------------------------

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

/// Why [`WorkspaceManager::verify_worktree`] refused to reuse a worktree.
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

/// What a worktree has to hold for [`WorkspaceManager::verify_worktree`] to
/// pass it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Quiescence {
    /// The ordinary case: HEAD equals the recorded base.
    AtBase(String),
    /// `RetainedIdle`: "the worktree holds the retained cumulative tree".
    HoldsTree(String),
}

/// The owner of an execution root and everything inside it.
#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    base: PathBuf,
    common_git_dir: PathBuf,
    repo_key: String,
    run_id: String,
    incarnation: String,
    /// The operator's authorized private root, canonicalized. It is the anchor
    /// the reparse-point walk starts at — see [`reparse_point_below`].
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
    /// `decisions.workspace_candidates.execution_root` names.
    ///
    /// # Errors
    ///
    /// [`Refusal::BaseIsNotADirectory`], [`Refusal::ReparsePointOnChain`],
    /// [`Refusal::RootInsideRepositoryWorktree`], and
    /// [`Refusal::WorktreeInsideRoot`], plus a Git error when the base is not a
    /// repository.
    pub fn derive(
        base: &Path,
        private_root: &Path,
        run_id: &str,
        incarnation: &str,
    ) -> Result<Self, UpstrokeError> {
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
        refuse_unreal_directory(&self.base)?;
        refuse_reparse_points(&self.private_root, &self.execution_root)?;
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
                for directory in [
                    self.execution_root.clone(),
                    self.execution_root.join("intents"),
                    self.execution_root.join("tasks"),
                    self.execution_root.join("merge"),
                    self.execution_root.join("snapshots"),
                    self.hooks_dir(),
                ] {
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
                if !self.execution_root.exists() {
                    return Ok(false);
                }
                for scaffolding in [
                    self.hooks_dir(),
                    self.execution_root.join("intents"),
                    self.execution_root.join("tasks"),
                    self.execution_root.join("merge"),
                    self.execution_root.join("snapshots"),
                ] {
                    if directory_is_empty(&scaffolding)? {
                        let _ = fs::remove_dir(&scaffolding);
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
        let record = IntentRecord {
            kind: slot.kind().to_owned(),
            slot: slot.relative().to_string_lossy().replace('\\', "/"),
            run_id: self.run_id.clone(),
            incarnation: self.incarnation.clone(),
        };
        let ledger = hooks.durability_ledger();
        funnel(hooks, slot.write_intent_site(), || {
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
        let path = self.intent_path(slot);
        let ledger = hooks.durability_ledger();
        funnel(hooks, slot.remove_intent_site(), || {
            match fs::remove_file(&path) {
                Ok(()) => sync_directory(path.parent().unwrap_or(&self.execution_root), &ledger),
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
        if !intent.is_file() {
            return Err(Refusal::AddWithoutIntent {
                slot: slot.relative().display().to_string(),
                intent,
            }
            .into());
        }
        funnel(hooks, slot.add_site(), || {
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
            argv.push(path.clone().into_os_string());
            argv.push(OsString::from(commit));
            self.git_ok(&self.base, &argv)?;
            Ok(path.clone())
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
            if path.exists() {
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
                if fs::metadata(admin.join("commondir")).is_ok_and(|metadata| metadata.len() == 0) {
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
            || self.git_line(&path, &Self::CANDIDATE_WRITE_TREE_ARGV),
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

    /// Run Git in `cwd` with every repository hook and the fsmonitor disabled.
    fn git(&self, cwd: &Path, args: &[OsString]) -> Result<Output, UpstrokeError> {
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
        refuse_unreal_directory(&self.base)?;
        refuse_reparse_points(&self.private_root, &self.execution_root)?;
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !target.exists() => {
                return Ok(None);
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

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// Decode the authoritative checkout side of a linked-worktree registration.
///
/// Registration-state table used by recovery:
///
/// | `gitdir` state | Can bind an exact checkout? | Recovery action |
/// |---|---:|---|
/// | valid UTF-8 or Unix path bytes | yes | revalidate containment, then act |
/// | absent or unreadable | no | refuse before mutation |
/// | zero-length | no | refuse before mutation |
/// | partial / not ending in `.git` | no | refuse before mutation |
///
/// `commondir` is deliberately not an input to this binding. A valid `gitdir`
/// plus an empty `commondir` is the one safe repairable state: it identifies
/// the checkout while explaining why Git's own enumeration cannot proceed.
fn registration_checkout(admin: &Path, bytes: &[u8]) -> Result<PathBuf, UpstrokeError> {
    let bytes = trim_ascii(bytes);
    if bytes.is_empty() {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has an empty gitdir",
                admin.display()
            ),
        });
    }
    let Some(recorded) = decode_registration_path(bytes) else {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir this platform cannot represent exactly",
                admin.display()
            ),
        });
    };
    let normalized: PathBuf = recorded.components().collect();
    if !recorded.is_absolute()
        || recorded
            .components()
            .any(|component| component == Component::ParentDir)
        || normalized.as_os_str() != recorded.as_os_str()
    {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir that is not an absolute normalized path",
                admin.display()
            ),
        });
    }
    if recorded.file_name() != Some(OsStr::new(".git")) {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a gitdir that does not name a checkout .git",
                admin.display()
            ),
        });
    }
    let Some(checkout) = recorded.parent() else {
        return Err(UpstrokeError::Git {
            message: format!(
                "worktree registration {} has a parentless gitdir",
                admin.display()
            ),
        });
    };
    Ok(checkout.to_path_buf())
}

#[cfg(unix)]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    Some(decode_git_path(bytes))
}

#[cfg(windows)]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes)
        .ok()
        .map(|path| PathBuf::from(path.replace('/', "\\")))
}

#[cfg(not(any(unix, windows)))]
fn decode_registration_path(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

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

// ---------------------------------------------------------------------------
// Residue classification
// ---------------------------------------------------------------------------

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
fn administrative_residue_at(git_dir: &Path) -> Result<Vec<ResidueElement>, UpstrokeError> {
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

/// Whether `value` is a full hexadecimal object id of either hash length.
#[must_use]
pub fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether `value` is the null object id of either hash length.
#[must_use]
pub fn is_null_object_id(value: &str) -> bool {
    is_object_id(value) && value.bytes().all(|byte| byte == b'0')
}

fn refuse_malformed_object_id(
    refname: &str,
    role: &'static str,
    value: &str,
) -> Result<(), UpstrokeError> {
    if is_object_id(value) {
        return Ok(());
    }
    Err(Refusal::MalformedObjectId {
        refname: refname.to_owned(),
        role,
        value: value.to_owned(),
    }
    .into())
}

/// The expected-old side of every move and delete: a well-formed, non-null id.
fn refuse_expected_old(refname: &str, old: &str) -> Result<(), UpstrokeError> {
    refuse_malformed_object_id(refname, "expected-old", old)?;
    if is_null_object_id(old) {
        return Err(Refusal::NullExpectedOld {
            refname: refname.to_owned(),
        }
        .into());
    }
    Ok(())
}

/// Turn `git diff --name-status -M -z` bytes into a [`PathSet`].
///
/// A separate function from [`WorkspaceManager::changed_paths`] so the hostile
/// byte cases — an undecodable path, an embedded newline, a path that is
/// nothing but a delimiter — can be exercised on every platform rather than
/// only on the one whose filesystem can hold them.
///
/// # The record grammar
///
/// `-z --name-status` emits NUL-*terminated* fields, one status field followed
/// by the paths that status has: `A\0path\0`, `D\0path\0`, `M\0path\0`, and for
/// a detected rename or copy **two** — `R100\0old\0new\0`. Both are kept, which
/// is `path_policy.actual`'s "both rename endpoints": the old endpoint is the
/// one another owner may already hold a lease on, and an answer that omits it
/// is silently smaller than the diff.
///
/// # Why unparsable is repo-wide, not shorter
///
/// One undecodable path makes the **whole** answer [`PathSet::RepoWide`], and
/// so does a status field this grammar does not recognise. The alternative,
/// dropping it and returning the rest, would hand the merge queue a region that
/// is silently *smaller* than the diff and let two overlapping tasks run in
/// parallel; `GitPath`'s own contract is that "paths that did not decode are
/// never stored", and `prediction` classifies "unsafe or unparsable forms" as
/// repo-wide. Repo-wide overlaps everything, so it is the direction that
/// refuses rather than the one that admits.
#[must_use]
pub fn decode_changed_paths(bytes: &[u8]) -> PathSet {
    let mut paths = Vec::new();
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let Some(endpoints) = status_endpoints(status) else {
            return PathSet::RepoWide;
        };
        for _ in 0..endpoints {
            // A record that stops mid-way is a truncated answer, and a
            // truncated answer is a shorter region.
            let Some(field) = fields.next() else {
                return PathSet::RepoWide;
            };
            match std::str::from_utf8(field) {
                Ok(decoded) => paths.push(GitPath::from(decoded)),
                Err(_) => return PathSet::RepoWide,
            }
        }
    }
    paths.sort();
    paths.dedup();
    PathSet::Prefixes { paths }
}

/// How many path fields a `--name-status` status field is followed by, or
/// `None` when this is not a status field at all.
///
/// The letters are `git diff`'s own documented set. `R` and `C` carry a
/// similarity score and two endpoints; everything else carries one and no
/// score. Anything else — including a path that arrived where a status was
/// expected, which is what a decoder reading `--name-only` output would see —
/// is unparsable and makes the answer repo-wide.
fn status_endpoints(status: &[u8]) -> Option<usize> {
    let (letter, score) = status.split_first()?;
    match letter {
        b'R' | b'C' => score
            .iter()
            .all(u8::is_ascii_digit)
            .then_some(2)
            .filter(|_| !score.is_empty()),
        b'A' | b'D' | b'M' | b'T' | b'U' | b'X' => score.is_empty().then_some(1),
        _ => None,
    }
}

/// Parse `git worktree list --porcelain -z`.
///
/// Attributes are NUL-terminated and an empty attribute ends a record. Paths
/// are taken as bytes, because a repository path need not be UTF-8 on Unix.
fn parse_worktree_records(bytes: &[u8]) -> Vec<WorktreeRecord> {
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(record) = current.take() {
                records.push(record);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(WorktreeRecord {
                path: decode_git_path(path),
                head: None,
                branch: None,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let Some(record) = current.as_mut() else {
            continue;
        };
        let text = String::from_utf8_lossy(field);
        let text = text.trim_end();
        if let Some(head) = text.strip_prefix("HEAD ") {
            record.head = Some(head.to_owned());
        } else if let Some(branch) = text.strip_prefix("branch ") {
            record.branch = Some(branch.to_owned());
        } else if text == "locked" {
            record.locked = Some(String::new());
        } else if let Some(reason) = text.strip_prefix("locked ") {
            record.locked = Some(reason.to_owned());
        } else if text == "prunable" {
            record.prunable = Some(String::new());
        } else if let Some(reason) = text.strip_prefix("prunable ") {
            record.prunable = Some(reason.to_owned());
        }
    }
    if let Some(record) = current.take() {
        records.push(record);
    }
    records
}

#[cfg(unix)]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).replace('/', "\\"))
}

#[cfg(not(any(unix, windows)))]
fn decode_git_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

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
    let staged = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&staged).map_err(|source| UpstrokeError::Io {
            path: staged.clone(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| UpstrokeError::Io {
            path: staged.clone(),
            source,
        })?;
        sync_file_recorded(&file, &staged, ledger)?;
    }
    fs::rename(&staged, path).map_err(|source| UpstrokeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    ledger.record(
        DurableStep::Renamed,
        path,
        fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
    );
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
/// already reviews, and every one of them is `#[cfg(test)]`. This module adds
/// **no attribute**: it is nested inside this file and inherits the
/// module-level allow the allowlist already records for it, so the
/// allow-placement scan sees nothing new.
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
