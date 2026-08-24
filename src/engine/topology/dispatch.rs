//! Dispatch: `task_dispatched` → intent → add, and the reuse gate over it.
//!
//! Two ordering clauses live here and they point in opposite directions, which
//! is the whole reason they are one module.
//!
//! **O21 — `task_dispatched` before worktree intent and add.** The event is
//! written *first*, before anything on disk exists.
//! [`crate::topology::events::TaskDispatched`] says why in its own doc: "a
//! worktree created before the event that records it is a directory nothing in
//! the log accounts for; an event written before a worktree that then fails to
//! appear is a generation the next process closes". So a fresh dispatch is
//! `task_dispatched` → `Worktree.WriteIntent` → `Worktree.Add`, and nothing
//! observes the worktree in between.
//!
//! **O22 — `Worktree.Verify` before any reuse.** `Verify` guards **reuse**, not
//! creation. `decisions.workspace_candidates.generation` names its three
//! occasions exactly — "a worktree is reused across a process boundary or after
//! an interrupted Git command (OpenNoAttempt recreate-or-verify, repair
//! materialization, retry verification) **only after** Worktree.Verify" — and
//! all three are recovery, never the first create. **Two** of them are this
//! module's — [`verify_or_recreate`] and, through it,
//! [`resume_open_no_attempt`]. The third, the retry verification, is
//! [`super::settle::retry`]'s, because a retained worktree that fails is
//! *closed* rather than recreated (INV-06) and the closure lives with the
//! reservation it has to cancel. Putting `Verify` in front of
//! a fresh add would make
//! `residue_carrying_worktree_fails_verify_and_is_recreated` inexpressible:
//! there would be no state in which a worktree exists, carries residue, and is
//! then recreated, because the verify would have run before it existed.
//!
//! # What is not here
//!
//! The attempt is [`super::attempt`]'s. Selection, the ceiling and
//! `budget_exceeded` are `select.rs`'s. The append-error protocol is
//! `emit.rs`'s, which is why this module takes an [`EventEmitter`] rather than
//! an [`crate::events::log::EventLog`].

use std::path::PathBuf;

use crate::error::UpstrokeError;
use crate::events::RunOutcome;
use crate::topology::events::{
    CandidateRef, CommitSha, GenerationCloseReason, GenerationClosed, GenerationId,
    LeaseDisposition, LeaseGrant, TaskDispatched, TopologyEventBody,
};
use crate::topology::paths::PathSet;
use crate::topology::registry::TaskKey;
use crate::workspace_manager::{Quiescence, Slot, VerifyFailure, WorkspaceManager};

use super::seams::TopologyHooks;

// ---------------------------------------------------------------------------
// The emit seam
// ---------------------------------------------------------------------------

/// Where a durable schema-4 event goes.
///
/// `coordinator_integration.emit` is "build event → serialize → round-trip →
/// plan_transition → append the exact bytes through the Event funnel", plus the
/// append-error protocol when that append fails. All of it is **`emit.rs`'s**
/// (O17), which is a different module of this slice: `dispatch.rs` and
/// `attempt.rs` decide *what* is appended and *when* relative to the effects
/// around it, and nothing here may decide *how*.
///
/// So the ordering modules take this and never an
/// [`crate::events::log::EventLog`]. A module that held the log would hold the
/// append-error protocol with it — no fold mutation, no retry, no report from
/// memory, no cleanup, then the stable-prefix barrier — and there would be two
/// implementations of it, which is the duplication class this crate has already
/// paid for three times.
pub trait EventEmitter {
    /// Emit one durable event, or fail.
    ///
    /// # Errors
    ///
    /// Whatever the emitter's append protocol returns. A caller here never
    /// interprets it: an emit that failed means the fold is poisoned and the
    /// coordinator is ending, and the effect that would have followed this
    /// event must not run.
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError>;
}

// ---------------------------------------------------------------------------
// What a dispatch is
// ---------------------------------------------------------------------------

/// Which of the two dispatches this is, and what each carries that the other
/// does not.
///
/// `decisions.admission_and_leases.leases.lifecycle`: "ordinary
/// `task_dispatched` creates the predicted lease (PR7) … a repair
/// `task_dispatched` records `InheritedLineage(root)`". The two are a sum
/// rather than a struct with two `Option`s because the fold refuses every
/// crossed combination — a predicted lease on a lineage member and an inherited
/// lease on an ordinary task are both `FoldError::MalformedEntry` — and a sum
/// makes the crossing unrepresentable instead of refused one layer later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchKind {
    /// An ordinary task, taking a predicted lease over the region its plan
    /// hints imply.
    Ordinary {
        /// The predicted region.
        paths: PathSet,
    },
    /// A repair, executing inside its lineage's lease and materializing the
    /// candidate that was rejected.
    Repair {
        /// The lineage root whose lease this generation executes inside.
        root: TaskKey,
        /// The protected candidate the worktree is materialized from.
        source: CandidateRef,
    },
}

impl DispatchKind {
    /// The grant `task_dispatched` records.
    fn grant(&self) -> LeaseGrant {
        match self {
            Self::Ordinary { paths } => LeaseGrant::Predicted {
                paths: paths.clone(),
            },
            Self::Repair { root, .. } => LeaseGrant::InheritedLineage { root: *root },
        }
    }

    /// The candidate `task_dispatched` records, for a repair.
    fn source_candidate(&self) -> Option<CandidateRef> {
        match self {
            Self::Ordinary { .. } => None,
            Self::Repair { source, .. } => Some(source.clone()),
        }
    }

    /// What a settlement of this generation records about the lease.
    ///
    /// Read off [`crate::topology::leases::GenerationLease::expected`]'s rule
    /// rather than restated: a repair never changes a lineage lease, so every
    /// one of its closures is `LineageHeld`, and an ordinary generation that
    /// closes releases the region it held.
    const fn closing_disposition(&self) -> LeaseDisposition {
        match self {
            Self::Ordinary { .. } => LeaseDisposition::PredictedReleased,
            Self::Repair { .. } => LeaseDisposition::LineageHeld,
        }
    }
}

/// One dispatched generation: exactly `T-DISPATCH`'s `durable_state`.
///
/// "generation, base, worktree path, lease relationship, source candidate for
/// repairs" — and every field here is one of those five. The worktree path is
/// carried as the [`Slot`] it was derived from rather than as a bare
/// [`PathBuf`], so a later reclaim re-derives the path from the execution root
/// and the slot name instead of trusting a string a record could carry out of
/// the root. `worktree` is the path the add returned, kept for assertions and
/// for the `worktree_path` the event recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    /// The task.
    pub key: TaskKey,
    /// Its generation.
    pub generation: GenerationId,
    /// The commit the worktree was created at.
    pub base: CommitSha,
    /// The slot the worktree occupies.
    pub slot: Slot,
    /// The checkout, at the path `Worktree.Add` returned for it — the slot
    /// target the funnel validated and handed to Git, not a reading of where
    /// Git put it.
    pub worktree: PathBuf,
    /// The lease relationship, and for a repair the candidate to materialize.
    pub kind: DispatchKind,
}

impl Dispatched {
    /// The quiescence a reuse of this worktree is checked against.
    ///
    /// [`Quiescence::AtBase`] and not [`Quiescence::HoldsTree`]: this generation
    /// has no attempt and therefore no cumulative tree — `HoldsTree` is
    /// `RetainedIdle`'s, and `settle.rs` owns that.
    #[must_use]
    pub fn quiescence(&self) -> Quiescence {
        Quiescence::AtBase(self.base.0.clone())
    }

    /// The narrower value the rebuild family takes.
    ///
    /// One conversion, at the two call sites that hold a `Dispatched`, so the
    /// rebuild path has a single parameter type and recovery does not need to
    /// build a dispatch it cannot prove.
    #[must_use]
    pub fn open_generation(&self) -> OpenGeneration {
        OpenGeneration {
            key: self.key,
            generation: self.generation,
            base: self.base.clone(),
            slot: self.slot.clone(),
            source: self.source().cloned(),
        }
    }

    /// What a settlement that **closes** this generation records about the
    /// lease.
    ///
    /// One reading of `GenerationLease::expected(false)` for both terminals
    /// this slice appends — `generation_closed{RunEnding}` here and
    /// `attempt_interrupted` in [`super::attempt`] — because the fold checks
    /// both against the same rule and two call sites deciding it separately is
    /// two chances to record `PredictedReleased` for a lineage member.
    #[must_use]
    pub const fn closing_disposition(&self) -> LeaseDisposition {
        self.kind.closing_disposition()
    }

    /// The candidate a repair materializes from.
    #[must_use]
    pub const fn source(&self) -> Option<&CandidateRef> {
        match &self.kind {
            DispatchKind::Ordinary { .. } => None,
            DispatchKind::Repair { source, .. } => Some(source),
        }
    }
}

/// What **rebuilding** an open generation needs, which is less than a dispatch.
///
/// [`Dispatched`] additionally carries the checkout path and the lease grant,
/// and no path below [`resume_open_no_attempt`] reads either: the rebuild
/// family asks the manager for `slot`, `base`, and — for a repair — the source
/// candidate, and nothing else.
///
/// **The narrowing is not tidiness, it is what lets recovery step (g) exist
/// without inventing a field.** A `Dispatched` reconstructed from the fold
/// would have to carry a `LeaseGrant`, whose predicted region the fold does not
/// hand back; a region invented at that call site is a field that lies about a
/// lease, and reaching into `src/topology/`'s lease table for the real one is
/// an edit to a frozen layer for a value the operation never reads. Asking for
/// what is used removes the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenGeneration {
    /// The task.
    pub key: TaskKey,
    /// Its generation.
    pub generation: GenerationId,
    /// The commit the worktree was created at, and is recreated at.
    pub base: CommitSha,
    /// The slot the worktree occupies.
    pub slot: Slot,
    /// For a repair, the protected candidate the worktree is materialized
    /// from. `None` for an ordinary generation.
    pub source: Option<CandidateRef>,
}

impl OpenGeneration {
    /// The quiescence a reuse of this worktree is checked against.
    ///
    /// [`Quiescence::AtBase`] and not [`Quiescence::HoldsTree`]: this
    /// generation has no attempt and therefore no cumulative tree.
    /// `HoldsTree` is `RetainedIdle`'s, and `settle.rs` owns that.
    #[must_use]
    pub fn quiescence(&self) -> Quiescence {
        Quiescence::AtBase(self.base.0.clone())
    }
}

/// The slot one generation of one task occupies.
///
/// `decisions.workspace_candidates.manager`: "detached linked worktrees with
/// durable synced intents (`tasks/k<key>-g<gen>`, `merge/s<seq>`)". Derived
/// here rather than at each call site so no two callers can disagree about
/// which worktree a generation owns.
#[must_use]
pub fn task_slot(key: TaskKey, generation: GenerationId) -> Slot {
    Slot::Task {
        key: key.0.to_string(),
        generation: generation.0,
    }
}

// ---------------------------------------------------------------------------
// O21 — the fresh dispatch
// ---------------------------------------------------------------------------

/// **O21.** Append `task_dispatched`, then write the intent, then add the
/// worktree — and for a repair, materialize its source into it.
///
/// The provisional reservation is *not* taken here. `permits.
/// provisional_reservations` puts it at "a selection decision", which is
/// `select.rs`'s, and has it "converted at the append" — so the conversion is
/// the caller's statement that this append happened, made with the same
/// `(key, kind)` it reserved under. Doing it inside would let a dispatch
/// convert a reservation nothing took.
///
/// There is **no `Worktree.Verify` in this function**, and that is O22 rather
/// than an omission: `Verify` guards reuse. See the module docs.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] when a repair's source candidate is not an object
/// this repository has — `T-DISPATCH`'s `refusal_condition`, and it is checked
/// *before* the append so the refusal costs no durable state. The containment
/// refusals of [`WorkspaceManager`] (a worktree path outside the execution root
/// or on a reparse point) are the other half of that condition and are raised by
/// the funnels themselves. Otherwise: whatever the emitter or a Git funnel
/// returns.
pub fn dispatch(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    emitter: &mut dyn EventEmitter,
    request: &DispatchRequest,
) -> Result<Dispatched, UpstrokeError> {
    // Before the append, because a refusal after it would leave an open
    // generation whose worktree can never be built. `T-DISPATCH` lists "source
    // candidate object missing" beside the containment refusals, so both are
    // here and both are ahead of the event.
    //
    // The containment half needs this call to be true of it at all.
    // `execution_root` says "every create/reclaim/delete revalidates", and
    // `write_intent` and `add_worktree` each do — but that is *after* this
    // function has appended, so without the line below the sentence above
    // would be a claim about the two effects rather than about the dispatch.
    // The three conditions are filesystem facts about the world and not about
    // this request: a foreign worktree created inside the execution root, a
    // reparse point planted on the chain, or a managed base that stopped being
    // a real directory can each start being true between `run_started` and
    // this call. It costs one read-only `git worktree list`; what it buys is
    // that the `OpenNoAttempt` generation this event opens is never one whose
    // worktree the very next line refuses to build — which is exactly what
    // hoisting `refuse_absent_source` above the append avoids for the other
    // half of the same `refusal_condition`.
    manager.revalidate()?;
    if let DispatchKind::Repair { source, .. } = &request.kind {
        refuse_absent_source(manager, source)?;
    }

    let slot = task_slot(request.key, request.generation);
    let worktree = manager.slot_path(&slot);

    // (1) The event. First, and by itself.
    emitter.emit(TopologyEventBody::TaskDispatched {
        data: TaskDispatched {
            key: request.key,
            generation: request.generation,
            base_sha: request.base.clone(),
            // The string a later process compares and re-derives. A platform
            // path type here would make a log written on one operating system a
            // question on another — `TaskDispatched::worktree_path` says so.
            worktree_path: worktree.to_string_lossy().into_owned(),
            lease: request.kind.grant(),
            source_candidate: request.kind.source_candidate(),
        },
    })?;

    // (2) The intent, synced. (3) The add, which refuses without it.
    let mut dispatched = Dispatched {
        key: request.key,
        generation: request.generation,
        base: request.base.clone(),
        slot,
        worktree,
        kind: request.kind.clone(),
    };
    // The add's own answer replaces the derivation, and what that buys is
    // narrow enough to be worth stating exactly. Until this line the field and
    // the event's `worktree_path` were one local under two names, so a test
    // comparing them compared a value to itself and could only fail on a lossy
    // conversion. Now the event's string is the pre-append derivation — O21
    // puts the append first, so it can be nothing else — and the field is what
    // `Worktree.Add` returned, which is the target the funnel validated and
    // handed to `git worktree add`.
    //
    // So their agreement says the durable event names the directory the add was
    // told to create. It is **not** an observation of where Git put the
    // checkout: `WorkspaceManager::add_worktree` answers with `slot_target`,
    // which is the same `slot_path` rule the string above came from, and
    // nothing reads the location back. A second, independent provenance would
    // have to come from `git worktree list`; it is owed, not claimed here.
    dispatched.worktree = create_worktree(manager, hooks, &dispatched.open_generation())?;

    // (4) A repair's materialization, which `ObjectSite::RepairMaterialize`
    // itself places `Adjacent::After(DurableEvent::TaskDispatched)`.
    if dispatched.source().is_some() {
        materialize_repair(manager, hooks, &dispatched.open_generation())?;
    }
    Ok(dispatched)
}

/// What a caller asks [`dispatch`] for.
///
/// A struct rather than four parameters because three of them are identities
/// that must agree with one another and with the reservation the caller
/// converts; a positional call that transposed key and generation would type-
/// check under two newtypes over `u32` if either lost its wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRequest {
    /// The task being dispatched.
    pub key: TaskKey,
    /// The generation this opens. Dense per task: the fold refuses any other.
    pub generation: GenerationId,
    /// The commit the worktree is created at.
    pub base: CommitSha,
    /// Ordinary or repair.
    pub kind: DispatchKind,
}

/// Intent then add, which is the only order [`WorkspaceManager::add_worktree`]
/// permits.
///
/// `Refusal::AddWithoutIntent` enforces it at the add site, so this order is
/// checked rather than merely intended — an add whose intent is not already
/// durable creates a worktree `reclaim_intents` can never find.
///
/// The path returned is the funnel's answer rather than a derivation made
/// beside it, so a caller that records the checkout's path has one source for
/// it. That source is [`WorkspaceManager::add_worktree`], whose answer is the
/// validated slot target it gave `git worktree add` — where the checkout was
/// *asked* to go. Nothing here reads back where Git put it.
fn create_worktree(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<PathBuf, UpstrokeError> {
    manager.write_intent(hooks.effects(), &open.slot)?;
    manager.add_worktree(hooks.effects(), &open.slot, &open.base.0)
}

/// Refuse a repair whose protected source is not in this repository.
///
/// Two conditions, because the packet's "source candidate object missing" can
/// arrive either way and only one of them is about the object: the authoritative
/// ref `refs/upstroke/runs/<id>/candidates/<key>/<gen>` is what keeps the commit
/// reachable (R11, "never pruned while the run can resume"), so a ref that is
/// gone or that names a different commit is the same failure as an object that
/// was never written, and both would leave `git cherry-pick` to fail with a
/// message about a revision rather than about a lost candidate.
fn refuse_absent_source(
    manager: &WorkspaceManager,
    source: &CandidateRef,
) -> Result<(), UpstrokeError> {
    if !manager.object_exists(&source.commit_sha.0)? {
        return Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: its commit `{}` is not \
                 an object in this repository, and `T-DISPATCH` refuses a dispatch whose source \
                 candidate object is missing",
                source.generation.0, source.key, source.commit_sha.0
            ),
        });
    }
    match manager.direct_ref_target(&source.candidate_ref.0)? {
        Some(target) if target.eq_ignore_ascii_case(&source.commit_sha.0) => Ok(()),
        Some(target) => Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: `{}` names `{target}` \
                 and the recorded candidate is `{}`",
                source.generation.0, source.key, source.candidate_ref.0, source.commit_sha.0
            ),
        }),
        None => Err(UpstrokeError::Refused {
            message: format!(
                "refusing to dispatch a repair of candidate {} of task {}: its authoritative ref \
                 `{}` does not exist, and it is what keeps the candidate reachable",
                source.generation.0, source.key, source.candidate_ref.0
            ),
        }),
    }
}

// ---------------------------------------------------------------------------
// O22 — reuse, and only reuse, is gated by Verify
// ---------------------------------------------------------------------------

/// What [`verify_or_recreate`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reuse {
    /// The recorded worktree passed `Worktree.Verify` and is reused as it
    /// stands.
    Verified,
    /// It did not, and was removed with force and rebuilt. Carries the failure
    /// so a caller — and a test — can say *why* it was not reusable, rather
    /// than only that it was not.
    Recreated {
        /// What `Worktree.Verify` refused it for.
        failure: VerifyFailure,
    },
}

impl Reuse {
    /// Whether the worktree that came back is the one that was there.
    #[must_use]
    pub const fn reused(&self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// **O22.** `Worktree.Verify` the recorded worktree, or remove it with force
/// and recreate it (intent then add).
///
/// `T-DISPATCH`'s `resume_action`, word for word: "verify the worktree at the
/// recorded base with Worktree.Verify (linked worktree at the recorded path,
/// HEAD == base, index unlocked, no cherry-pick/merge/sequencer state) or remove
/// it with force and recreate it (intent then add)". Every one of those four
/// conditions is [`WorkspaceManager::verify_worktree`]'s, not restated here —
/// a second copy of a predicate is a second thing to keep in step, and this one
/// is `VerifyFailure`'s whole vocabulary.
///
/// # The quiescence is the caller's, not this function's
///
/// `Quiescence` has two forms and the packet gives both in one breath: "HEAD
/// equals the recorded base (**or, for RetainedIdle, the worktree holds the
/// retained cumulative tree**)". Which one applies is a property of the
/// generation's *class*, which lives in the fold — so it is a parameter here
/// rather than something derived from [`Dispatched`], which knows only that a
/// generation was opened. [`Dispatched::quiescence`] is the `OpenNoAttempt`
/// answer and is what [`resume_open_no_attempt`] passes.
///
/// [`Quiescence::HoldsTree`] does **not** belong to this function, and that is
/// not a matter of taste: it is `RetainedIdle`'s form of the check, and a
/// retained generation is closed rather than recreated (INV-06). No retained
/// worktree reaches this module at all — [`super::settle::retry`] performs the
/// retained `Worktree.Verify` through its own `WorktreeVerify` seam and writes
/// the closure itself — so nothing here has one to hand the recreate branch.
///
/// # The intent is re-written rather than removed and re-written
///
/// The reclaim order elsewhere is *worktree then intent*, because an intent that
/// outlives its worktree is reclaimed harmlessly while a worktree that outlives
/// its intent is a leak nothing can find. That reasoning applies here too, so
/// the sequence is: force-remove the worktree, re-write the (idempotent) intent,
/// add. At no instant does a worktree exist without a durable intent naming it.
///
/// # Errors
///
/// The containment refusals, or a Git or I/O error. A worktree that merely fails
/// its quiescence check is `Ok(Reuse::Recreated { .. })` — that is a decision,
/// not a failure.
pub fn verify_or_recreate(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
    quiescence: &Quiescence,
) -> Result<Reuse, UpstrokeError> {
    match verify_reuse(manager, hooks, open, quiescence)? {
        Ok(()) => Ok(Reuse::Verified),
        Err(failure) => {
            manager.remove_worktree(hooks.effects(), &open.slot)?;
            create_worktree(manager, hooks, open)?;
            Ok(Reuse::Recreated { failure })
        }
    }
}

/// **O22's observation, with no branch on it.** `Worktree.Verify` the recorded
/// worktree and report what it saw.
///
/// `decisions.workspace_candidates.generation` gives the failure two different
/// recoveries, and which one applies is a property of the generation's class,
/// not of the observation: "failing verification an OpenNoAttempt or repair
/// worktree is removed with force and recreated, and a **RetainedIdle
/// generation is closed** with `generation_closed{WorktreeMissing}`". INV-06
/// states the same thing as a prohibition — a retained generation "is never
/// recreated" — because what its worktree holds is a cumulative tree that no
/// base can be re-cut into, so a forced removal there destroys work
/// irrecoverably rather than costing a rebuild.
///
/// So the observation is one function and the recovery is the caller's.
/// [`verify_or_recreate`] is the rebuilding half and is for the two classes
/// that may be rebuilt. The retained class's recovery is
/// [`super::settle::retry`]'s and there is exactly one of it — it reaches
/// `Worktree.Verify` through its own `WorktreeVerify` seam rather than through
/// this function, so a retained worktree never arrives here to be handed the
/// recreate branch.
///
/// That leaves this function with one caller today, and it stays a separate
/// function rather than being folded back into [`verify_or_recreate`] because
/// what it separates is the *branch*, not the caller: the observation and the
/// destructive recovery are named apart, so a future retained-class reader in
/// this module has something to take that has no `remove_worktree` beyond it.
///
/// # Errors
///
/// The containment refusals, or a Git error. A worktree that merely fails its
/// quiescence check is `Ok(Err(VerifyFailure))` — that is an observation, not a
/// failure.
pub fn verify_reuse(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
    quiescence: &Quiescence,
) -> Result<Result<(), VerifyFailure>, UpstrokeError> {
    manager.verify_worktree(hooks.effects(), &open.slot, quiescence)
}

/// Re-run a repair's recorded materialization in a verified or fresh worktree.
///
/// `Object.RepairMaterialize` is `git cherry-pick --no-commit`, whose merge
/// objects are referenced by the worktree index (R9). It is idempotent only in
/// the sense that re-running it *in a worktree at the recorded base* reproduces
/// the same index — which is exactly why `T-DISPATCH` says "re-run the recorded
/// materialization in a **verified or fresh** worktree" and why a caller reaches
/// this through [`resume_open_no_attempt`] rather than directly.
///
/// # Errors
///
/// [`UpstrokeError::Refused`] for an ordinary dispatch, which has no
/// materialization to reproduce and whose caller has therefore lost track of
/// which kind it holds. Otherwise the containment refusals or a Git error.
pub fn materialize_repair(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<(), UpstrokeError> {
    let Some(source) = open.source.as_ref() else {
        return Err(UpstrokeError::Refused {
            message: format!(
                "task {} generation {} is an ordinary dispatch and has no recorded \
                 materialization to reproduce",
                open.key, open.generation.0
            ),
        });
    };
    manager.repair_materialize(hooks.effects(), &open.slot, &source.commit_sha.0)
}

/// The whole of `T-DISPATCH`'s resume action for a live process, in order.
///
/// Verify-or-recreate first, then — for a repair — the materialization, because
/// the materialization is what has to land in a worktree that is already known
/// good. A repair whose materialization *completed* before the kill leaves
/// `CHERRY_PICK_HEAD` in its git dir, which `Worktree.Verify` reads as
/// administrative residue and refuses, so such a worktree is recreated and
/// re-materialized rather than reused. That is convergent and deliberate: the
/// alternative is a verify that tries to decide whether a half-applied index is
/// the same half-applied index, which is not a question a read-only observation
/// can answer.
///
/// # Errors
///
/// As [`verify_or_recreate`] and [`materialize_repair`].
pub fn resume_open_no_attempt(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    open: &OpenGeneration,
) -> Result<Reuse, UpstrokeError> {
    let reuse = verify_or_recreate(manager, hooks, open, &open.quiescence())?;
    if open.source.is_some() {
        materialize_repair(manager, hooks, open)?;
    }
    Ok(reuse)
}

// ---------------------------------------------------------------------------
// Run end
// ---------------------------------------------------------------------------

/// Close an `OpenNoAttempt` generation at run end and scrub its worktree.
///
/// `decisions.workspace_candidates.generation`: "a generation with no attempt
/// started is recreated at its recorded base during a live run and **closed at
/// run end**", and `cleanup`: "RetainedIdle and OpenNoAttempt worktrees are
/// resumably_open during a live run and closed at run end".
///
/// The event precedes the scrub, and that is `cleanup`'s own rule — "task
/// worktree scrubbed only after `task_candidate_created` is durable **or the
/// generation is Closed**". A scrub before the close would remove a worktree the
/// log still says is resumably open, which is the state a resume would then try
/// to verify.
///
/// # Errors
///
/// Whatever the emitter returns, or a Git or I/O error from the scrub. The scrub
/// runs only if the append succeeded, for the reason above.
pub fn close_at_run_end(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    emitter: &mut dyn EventEmitter,
    dispatched: &Dispatched,
    outcome: RunOutcome,
) -> Result<(), UpstrokeError> {
    emitter.emit(TopologyEventBody::GenerationClosed {
        data: GenerationClosed {
            key: dispatched.key,
            generation: dispatched.generation,
            reason: GenerationCloseReason::RunEnding { outcome },
            lease: dispatched.kind.closing_disposition(),
        },
    })?;
    scrub(manager, hooks, &dispatched.slot)
}

/// Forced removal of a worktree and then its intent.
///
/// `cleanup`: "every worktree, staging, and snapshot removal is forced … so Git
/// administrative residue left by an interrupted command … never blocks
/// reclaim". Worktree first, then intent, so that the durable record naming the
/// worktree outlives the worktree rather than the other way round.
pub(super) fn scrub(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    slot: &Slot,
) -> Result<(), UpstrokeError> {
    manager.remove_worktree(hooks.effects(), slot)?;
    manager.remove_intent(hooks.effects(), slot)
}

#[cfg(test)]
mod tests;
