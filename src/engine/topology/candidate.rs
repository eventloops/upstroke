//! The sequence from a judged tree to a queued candidate.
//!
//! `decisions.workspace_candidates.candidate` gives the order, and it is the
//! whole of what this module is:
//!
//! ```text
//! hook-free commit-tree (Object.CandidateCommitTree: the commit is
//!   unreferenced Git/GC-owned residue, R27, until pinned; the parent-executed
//!   IdUnread point lies between the child's exit and the coordinator
//!   recording the id)
//! -> pin candidate-prepared/<key>/<gen> zero-old (R23)
//! -> append candidate_prepared (Promoting; candidate lease or lineage widening)
//! -> create candidates/<key>/<gen> zero-old (R11)
//! -> append task_candidate_created (queue position; Promoting ends; pipeline
//!    entitlement released)
//! -> prune the pin expected-old
//! ```
//!
//! …and then `cleanup`'s scrub: "task worktree scrubbed only after
//! `task_candidate_created` is durable or the generation is Closed".
//!
//! Six steps, four ordering clauses — O28 commit object before pin with
//! `IdUnread` between, O29 pin before `candidate_prepared`, O30 the candidates
//! ref after `candidate_prepared` and before `task_candidate_created`, O31 pin
//! pruning and the forced scrub after `task_candidate_created`.
//!
//! # The order is a typestate, not a comment
//!
//! Each step takes the previous step's witness **by value** and returns its
//! own. [`UnpinnedCandidate`] is the only thing [`pin_candidate`] accepts,
//! [`PinnedCandidate`] the only thing [`append_candidate_prepared`] accepts,
//! and so on. A caller cannot append `candidate_prepared` for a commit it never
//! pinned, or create the candidates ref before the prepare, because there is no
//! value of the right type to pass. That is the shape
//! `crate::engine::topology`'s module documentation says three of this slice's
//! rules are being built into, and this is one of them.
//!
//! The witnesses are also why the sequence is six functions rather than one:
//! every step is a separate obligation and a separate durable prefix, and the
//! fault matrix tables a resume action for each. None of them derives `Clone`:
//! a cloned [`PinnedCandidate`] is two `candidate_prepared` appends for one
//! pin, and a rule the fold has to catch at runtime is worse than one the type
//! checker refuses to compile.
//!
//! # The settlement lands between step 2 and step 3, and is not this module's
//!
//! There is no function here that runs the sequence from a tree to a queue
//! position, and that is not an omission.
//! `transaction_fault_matrix[T-CAND-OBJ]` puts the whole of steps 1 and 2 in a
//! window whose `durable_state` is "attempt_started only" and whose
//! `authoritative_state` is "**attempt unsettled**" — so the commit object and
//! its pin are written while the attempt is still unsettled, and the settlement
//! is `candidate_prepared` itself.
//!
//! **This said `attempt_finished(succeeded)` is appended between the pin and
//! `candidate_prepared`, "which is what makes the generation `Promoting`".** It
//! is not, and since the 2026-08-27 CONFORM ruling the fold refuses that event:
//! `decisions/2026-08-12-merge-queue-execution-topology.md` makes
//! `candidate_prepared` the sole successful settlement for a candidate-producing
//! attempt and adds that `attempt_finished` "is not also emitted for that
//! attempt". `apply_candidate_prepared` is what promotes the generation.
//!
//! That is the whole reason a resume in this window "settles attempt
//! interrupted" rather than promoting: there is no settled attempt to prepare a
//! candidate for, and the objects the run wrote are Git's.
//!
//! [`promote`] therefore starts at the pin — steps 3 to 6, everything after
//! the settlement.
//!
//! # Two claims this module exists to keep
//!
//! **An unpinned candidate commit is never adopted.**
//! `transaction_fault_matrix[T-CAND-OBJ].resume_action` for the prefix where
//! the object exists and no pin does is "nothing to delete: the unpinned object
//! is left to Git (never adopted; decision:295)". [`recovery_for`] returns
//! [`CandidateRecovery`] with `settles_interrupted` set there, naming no object,
//! so there is no value a later step could adopt. The commit stays unreachable
//! and Git's garbage collector owns it (R27).
//!
//! **The pin is pruned right after promotion; the candidates ref is not.**
//! `cleanup`: "candidate-prepared pins pruned right after promotion (or as
//! orphans); … candidates refs (R11) **never pruned while the run can
//! resume**, pruned only by Complete finalization, retained as forensic output
//! at Halted". So [`complete_promotion`] deletes the pin and nothing here ever
//! deletes a candidates ref.
//!
//! # What guards these refs, and what does not
//!
//! `transaction_fault_matrix[T-CAND-OBJ].refusal_condition` is "pin symbolic or
//! an unexpected ref under the run namespace". The first half is
//! [`WorkspaceManager`]'s: every ref primitive this module calls refuses a
//! symbolic name before touching it, which is `ref_rules`' "symbolic refs
//! refused". The second half needs a list somebody derived, and that is
//! [`expected_refs`].
//!
//! `WorkspaceManager::assert_publishable` is **not** called here, and that is
//! not an omission. `decisions.workspace_candidates.integration_ref` is what
//! asks for it — "`refs/heads/upstroke/run-<id>` … `assert_publishable()`
//! before every prepare/CAS/recovery" — and its second conjunct, "is this ref
//! checked out in a worktree?", can only ever answer yes for a ref under
//! `refs/heads/`. Neither of this module's two refs is one, and neither is ever
//! checked out. Calling it would add a Git subprocess per candidate to answer a
//! question whose answer is fixed by where the ref lives.
//!
//! # What "no production path" means here, precisely
//!
//! `decisions.pr_sequence[8].production_effect` is "none", and that is true of
//! the **shipped binary**: `upstroke run` drives the legacy coordinator and
//! reaches nothing in this file.
//!
//! It stopped being true as written. This section said "nothing here is a
//! production path yet … the schema-4 coordinator that will call them is the
//! rest of PR7", and that coordinator **arrived in this slice**:
//! `TopologyRun::promote_candidate` calls `write_candidate_commit`,
//! `pin_candidate`, `append_candidate_prepared`, `create_candidates_ref`,
//! `append_candidate_created` and `reclaim_after_creation`, and
//! `recover::finish_promotions` calls the last three. They are non-`#[cfg(test)]`
//! callers; what keeps the effect "none" is that `engine::topology` is
//! `pub(crate)` and no shipped command drives it, not that the callers do not
//! exist. Frontier review of `75da796`, finding 5.

use std::path::Path;

use thiserror::Error;

use super::seams::TopologyHooks;
use crate::error::UpstrokeError;
use crate::events::AttemptRecord;
use crate::topology::effects::{EffectSiteId, ObjectResidue, ObjectSite, RefSite};
use crate::topology::events::{
    CandidateLeaseEffect, CandidatePrepared, CandidateRef, CommitSha, GenerationId, GitRef,
    TaskCandidateCreated, TopologyEventBody,
};
use crate::topology::fold::{GenerationClass, TopologyFold};
use crate::topology::paths::PathSet;
use crate::topology::registry::TaskKey;
use crate::workspace_manager::{
    ResidueTarget, Slot, WorkspaceManager, classify_object_residue, is_object_id,
};

// ---------------------------------------------------------------------------
// The run ref namespace
// ---------------------------------------------------------------------------

/// The root every run-scoped engine ref hangs from.
///
/// `decisions.workspace_candidates.refs` writes all three of them under
/// `refs/upstroke/runs/<id>/`; the integration ref is the one that lives
/// elsewhere, under `refs/heads/`.
pub const RUN_REF_ROOT: &str = "refs/upstroke/runs";

/// The namespace of one run's refs, with the trailing separator a prefix match
/// needs.
///
/// The separator is not decoration: without it the namespace of run `abc` also
/// matches run `abcd`'s refs, and the unexpected-ref refusal would then read a
/// sibling run's namespace as this one's contamination.
#[must_use]
pub fn run_namespace(run_id: &str) -> String {
    format!("{RUN_REF_ROOT}/{run_id}/")
}

/// `refs/upstroke/runs/<id>/candidate-prepared/<key>/<gen>` — the pin (R23).
#[must_use]
pub fn candidate_pin_ref(run_id: &str, key: TaskKey, generation: GenerationId) -> GitRef {
    GitRef(format!(
        "{RUN_REF_ROOT}/{run_id}/candidate-prepared/{}/{}",
        key.0, generation.0
    ))
}

/// `refs/upstroke/runs/<id>/candidates/<key>/<gen>` — the authoritative ref
/// (R11).
#[must_use]
pub fn candidates_ref(run_id: &str, key: TaskKey, generation: GenerationId) -> GitRef {
    GitRef(format!(
        "{RUN_REF_ROOT}/{run_id}/candidates/{}/{}",
        key.0, generation.0
    ))
}

/// The two names one candidate takes, derived together.
///
/// **`<key>` is the numeric [`TaskKey`], not the task's display id.** Both
/// appear in the packet's examples, and only one of them is safe: a display id
/// comes from a plan file and may carry a `/`, a `..`, a trailing `.lock`, or
/// any of the other shapes `git check-ref-format` refuses, so a ref built from
/// one is a ref creation that fails on somebody's plan. `TaskKey` is a `u32`
/// and renders to digits, which are legal in every position of a ref. The fold
/// compares these names for equality and never parses them, so nothing downs-
/// tream depends on which of the two is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateNames {
    /// The pin, R23.
    pub prepared_ref: GitRef,
    /// The authoritative ref, R11.
    pub candidate_ref: GitRef,
}

impl CandidateNames {
    /// The names of one generation's candidate.
    #[must_use]
    pub fn of(run_id: &str, key: TaskKey, generation: GenerationId) -> Self {
        Self {
            prepared_ref: candidate_pin_ref(run_id, key, generation),
            candidate_ref: candidates_ref(run_id, key, generation),
        }
    }
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// What this sequence refuses, as values rather than as prose.
///
/// `transaction_fault_matrix[T-CAND-OBJ].refusal_condition` is "pin symbolic or
/// an unexpected ref under the run namespace" — both of which
/// [`WorkspaceManager`] already owns — and
/// `transaction_fault_matrix[T-CAND-REF].refusal_condition` is "object missing
/// or different; ref present at another SHA", which is this enum.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Refusal {
    /// `T-CAND-REF`: "object missing".
    #[error(
        "refusing to promote `{key}`/`{generation}`: its candidate commit {commit} is not an \
         object in this repository, and a candidates ref may only be created for the exact commit \
         `candidate_prepared` recorded"
    )]
    ObjectMissing {
        /// The task.
        key: u32,
        /// Its generation.
        generation: u32,
        /// The commit the durable record names.
        commit: String,
    },

    /// `T-CAND-REF`: "ref present at another SHA".
    ///
    /// Serves both refs of the candidate sequence: the authoritative candidates
    /// ref, which is never moved (R11), and the prepared **pin**, which binds to
    /// the commit `candidate_prepared` recorded. A substituted pin refuses here
    /// rather than being pruned, because deleting it is deleting the evidence
    /// that it was substituted — DESIGN §15's "refuses while preserving
    /// evidence".
    #[error(
        "refusing to promote `{refname}`: it is present at {found} and `candidate_prepared` \
         recorded {expected}. The candidates ref is authoritative (R11) and is never moved, \
         and the prepared pin binds to the recorded commit; neither is removed on a \
         mismatch, so the substitution stays visible"
    )]
    RefAtAnotherSha {
        /// The ref.
        refname: String,
        /// What it points at now.
        found: String,
        /// What the durable record says it must point at.
        expected: String,
    },

    /// A commit id that is not a full hexadecimal object id. Refused before any
    /// ref primitive sees it, because `git update-ref` reads a short or
    /// malformed value as a name to resolve rather than as an error.
    #[error(
        "refusing `{value}` as the candidate commit of `{key}`/`{generation}`: the sequence takes \
         a full hexadecimal object id"
    )]
    MalformedCommit {
        /// The task.
        key: u32,
        /// Its generation.
        generation: u32,
        /// The value as it was offered.
        value: String,
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
// The seam: the durable append
// ---------------------------------------------------------------------------

/// The one thing this sequence does not own.
///
/// `coordinator_integration.emit` is "build event → serialize → round-trip →
/// `plan_transition` → append the exact bytes through the Event funnel", and
/// the append-error protocol that guards it is `emit`'s, not this module's
/// (O17). So the two appends of the candidate sequence go through a seam, and
/// the coordinator's emitter is what satisfies it.
///
/// [`Self::fold`] is here rather than passed separately because every question
/// this module asks of live state — is this generation still `Promoting`? has
/// its `task_candidate_created` already landed? — is a question about the state
/// the emitter just folded. Two values that could disagree would be two
/// answers.
pub trait CandidateJournal {
    /// Check, append durably, and fold `body`.
    ///
    /// # Errors
    ///
    /// A refused transition, or any failure of the append funnel.
    fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError>;

    /// The live fold, after everything this journal has emitted.
    fn fold(&self) -> &TopologyFold;
}

// ---------------------------------------------------------------------------
// The witnesses
// ---------------------------------------------------------------------------

/// What a judged tree hands the sequence.
///
/// Every field is a field of `candidate_prepared`
/// (`decisions.workspace_candidates.candidate`) except the two the sequence
/// derives for itself: `commit_sha`, which is what commit-tree prints, and
/// `parent_sha`, which is `base_sha` by `CandidatePrepared::parent_is_base` and
/// is therefore not a second input a caller could get wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgedTree {
    /// The task.
    pub key: TaskKey,
    /// Its generation.
    pub generation: GenerationId,
    /// The attempt whose gates and reviewers judged this tree.
    pub attempt: Box<AttemptRecord>,
    /// The commit the worktree was created at, and the commit the candidate is
    /// parented on.
    pub base_sha: CommitSha,
    /// The exact tree under judgment.
    pub tree_sha: CommitSha,
    /// The candidate commit's message.
    pub message: String,
    /// The region the diff actually touched.
    pub actual_paths: PathSet,
    /// What the candidate does to the generation's lease (INV-16).
    pub lease_effect: CandidateLeaseEffect,
}

/// **Step 1 is done: the commit object exists and nothing references it.**
///
/// `resource_accounting[R27]`, and `candidate`'s own words: "the commit is
/// unreferenced Git/GC-owned residue, R27, until pinned". Dropping this value
/// without pinning is not a leak the engine has to clean up — it is the
/// tabled outcome, and Git's garbage collector owns what is left.
#[derive(Debug, PartialEq)]
#[must_use = "an unpinned candidate commit is Git's until `pin_candidate` takes it"]
pub struct UnpinnedCandidate {
    judged: JudgedTree,
    names: CandidateNames,
    commit_sha: CommitSha,
}

impl UnpinnedCandidate {
    /// The commit the coordinator recorded.
    #[must_use]
    pub fn commit_sha(&self) -> &CommitSha {
        &self.commit_sha
    }
}

/// **Step 2 is done: the pin exists (R23) and references the commit.**
#[derive(Debug, PartialEq)]
#[must_use = "a pin with no `candidate_prepared` is the orphan a resume prunes"]
pub struct PinnedCandidate {
    judged: JudgedTree,
    names: CandidateNames,
    commit_sha: CommitSha,
}

/// **Step 3 is done: `candidate_prepared` is durable and the generation is
/// `Promoting`.**
///
/// `candidate`: "a Promoting generation is always promoted before any
/// `run_finished`". This value is what the closure procedure at run end and the
/// resume in `transaction_fault_matrix[T-CAND-REF]` both hold — which is why
/// [`recovery_for`] hands one back rather than describing one.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a Promoting generation is always promoted before any run_finished"]
pub struct PromotingCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
    base: CommitSha,
    /// The tree the attempt's gates and reviewers judged, from the durable
    /// record. Carried beside the base because adoption checks both.
    tree: CommitSha,
}

impl PromotingCandidate {
    /// The candidate as the merge queue names it.
    #[must_use]
    pub fn candidate(&self) -> &CandidateRef {
        &self.candidate
    }

    /// The pin that is still holding the commit.
    #[must_use]
    pub fn prepared_ref(&self) -> &GitRef {
        &self.prepared_ref
    }

    /// The base the generation was dispatched at — the candidate's parent.
    ///
    /// Carried alongside the candidate rather than re-derived because the
    /// promotion is what verifies the object, and verifying it means comparing
    /// it against the base the record already committed to. Both producers have
    /// it in hand: the append built `parent_sha` from it, and the recovery reads
    /// it off the durable `PreparedCandidate`.
    #[must_use]
    #[allow(dead_code)] // never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)
    pub fn base(&self) -> &CommitSha {
        &self.base
    }
}

/// **The sequence is done: the candidate holds a queue position, the pin is
/// pruned, and the task worktree is scrubbed.**
#[derive(Debug, PartialEq, Eq)]
pub struct QueuedCandidate {
    candidate: CandidateRef,
}

impl QueuedCandidate {
    /// The candidate as the merge queue names it.
    #[must_use]
    pub fn candidate(&self) -> &CandidateRef {
        &self.candidate
    }
}

// ---------------------------------------------------------------------------
// The sequence
// ---------------------------------------------------------------------------

/// **O28, first half.** `Object.CandidateCommitTree`: write the candidate
/// commit.
///
/// The commit is unreferenced when this returns (R27), and the parent-executed
/// `IdUnread` point — the child exited with the object written, the coordinator
/// has not recorded the printed id — lies inside the funnel, between those two
/// facts. [`WorkspaceManager::candidate_commit_tree`] is what exposes it; this
/// function adds no point of its own, because a point the frozen inventory does
/// not declare is a coverage coordinate that measures nothing.
///
/// # Errors
///
/// A Git error from `commit-tree`, or an injected fault at the site.
pub fn write_candidate_commit(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    run_id: &str,
    judged: JudgedTree,
) -> Result<UnpinnedCandidate, UpstrokeError> {
    let names = CandidateNames::of(run_id, judged.key, judged.generation);
    let commit = manager.candidate_commit_tree(
        hooks.effects(),
        judged.tree_sha.as_str(),
        judged.base_sha.as_str(),
        &judged.message,
    )?;
    let commit_sha = CommitSha(commit);
    refuse_malformed_commit(judged.key, judged.generation, &commit_sha)?;
    Ok(UnpinnedCandidate {
        judged,
        names,
        commit_sha,
    })
}

/// **O28, second half.** `Ref.PinCandidatePrepared`: pin the commit zero-old
/// (R23).
///
/// Zero-old and `--no-deref`, and a symbolic name refuses, because
/// `decisions.workspace_candidates.ref_rules` says every engine ref is created
/// that way and [`WorkspaceManager::create_ref_zero_old`] is where that lives.
///
/// # Errors
///
/// A symbolic pin, a pin that already exists, or a Git error.
pub fn pin_candidate(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    unpinned: UnpinnedCandidate,
) -> Result<PinnedCandidate, UpstrokeError> {
    let UnpinnedCandidate {
        judged,
        names,
        commit_sha,
    } = unpinned;
    manager.create_ref_zero_old(
        hooks.effects(),
        RefSite::PinCandidatePrepared,
        names.prepared_ref.as_str(),
        commit_sha.as_str(),
    )?;
    Ok(PinnedCandidate {
        judged,
        names,
        commit_sha,
    })
}

/// **O29.** Append `candidate_prepared`, which is what makes the generation
/// `Promoting`.
///
/// The record is assembled here rather than by the caller for the reason
/// `parent_sha` exists at all: the fold refuses a candidate whose parent is not
/// the base its generation was dispatched at
/// (`CandidatePrepared::parent_is_base`), so the two fields move together or
/// not at all.
///
/// # Errors
///
/// A refused transition, or any failure of the append funnel.
pub fn append_candidate_prepared(
    journal: &mut dyn CandidateJournal,
    pinned: PinnedCandidate,
) -> Result<PromotingCandidate, UpstrokeError> {
    let PinnedCandidate {
        judged,
        names,
        commit_sha,
    } = pinned;
    let prepared = CandidatePrepared {
        key: judged.key,
        generation: judged.generation,
        attempt: judged.attempt,
        base_sha: judged.base_sha.clone(),
        parent_sha: judged.base_sha.clone(),
        tree_sha: judged.tree_sha.clone(),
        commit_sha,
        message: judged.message,
        prepared_ref: names.prepared_ref.clone(),
        candidate_ref: names.candidate_ref,
        actual_paths: judged.actual_paths,
        lease_effect: judged.lease_effect,
    };
    let candidate = prepared.candidate();
    journal.emit(TopologyEventBody::CandidatePrepared {
        data: Box::new(prepared),
    })?;
    Ok(PromotingCandidate {
        candidate,
        prepared_ref: names.prepared_ref,
        base: judged.base_sha,
        // The same value the event just recorded, so the live path and a resume
        // verify against one number rather than two that agree by inspection.
        tree: judged.tree_sha,
    })
}

/// **O30 and O31.** The rest of the sequence, and the only path to it.
///
/// This is `transaction_fault_matrix[T-CAND-REF].resume_action` verbatim —
/// "verify object; create exact candidates ref zero-old if absent; append
/// `task_candidate_created`; prune the pin (no spend repeats)" — followed by
/// `T-SCRUB`'s "idempotent contained forced removal of the worktree and
/// intent". The same sentence adds "the closure procedure performs the same
/// steps at any run end", so the live path and the resume path are one
/// function; there is no second one to drift from it.
///
/// Every step is therefore idempotent and reads the world before acting:
///
/// * the candidates ref is created only when absent, accepted when already at
///   the recorded commit, and refused at any other (R11 is authoritative and is
///   never moved);
/// * `task_candidate_created` is appended only while the generation is still
///   `Promoting`, so a second call after a successful append is a no-op rather
///   than a duplicate the fold would refuse;
/// * the pin is pruned expected-old only when it is present;
/// * the scrub is `WorkspaceManager::remove_worktree`, which is forced.
///
/// The candidates ref is **not** pruned here, at any run end.
/// `decisions.workspace_candidates.cleanup`: "candidates refs (R11) never
/// pruned while the run can resume, pruned only by Complete finalization,
/// retained as forensic output at Halted."
///
/// **Snapshots are not removed here either**, though `T-SCRUB`'s boundary
/// names them. `cleanup` gives them two mechanisms of their own — "snapshots
/// pruned on completion and reclaimed as residue" — and neither is this
/// function: a snapshot is pruned by the gate or review that finished with it,
/// and one left by a process that died is reclaimed from its intent by
/// [`WorkspaceManager::reclaim_intents`] at the next process start. That is
/// what `T-SCRUB`'s "snapshot intents reclaimed" names. Removing them from
/// here as well would be a second authority over the same rows.
///
/// # Errors
///
/// [`Refusal::ObjectMissing`], [`Refusal::RefAtAnotherSha`], a refused
/// transition, or a Git, I/O or append failure.
pub fn complete_promotion(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    journal: &mut dyn CandidateJournal,
    worktree: &Slot,
    promoting: PromotingCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let referenced = create_candidates_ref(manager, hooks, promoting)?;
    let created = append_candidate_created(journal, referenced)?;
    reclaim_after_creation(manager, hooks, worktree, created)
}

/// The candidate has its exact ref; `task_candidate_created` has not been
/// appended.
#[derive(Debug)]
#[must_use = "a candidate with its ref and no `task_candidate_created` is a \
              generation still Promoting"]
pub struct ReferencedCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
}

/// `task_candidate_created` is durable; the pin and the worktree are not yet
/// reclaimed.
#[derive(Debug)]
#[must_use = "a created candidate still owns its pin and its worktree"]
pub struct CreatedCandidate {
    candidate: CandidateRef,
    prepared_ref: GitRef,
}

/// **The effects half, before the append.** Verify the object, then create the
/// exact candidates ref zero-old if it is absent.
///
/// Split out of [`complete_promotion`] so a caller whose journal *is* its hooks
/// bundle can run the sequence at all. The three halves alternate between the
/// two — ref, append, reclaim — and a single `&mut dyn TopologyHooks` cannot be
/// held by the caller and by the journal at the same time. The typestate keeps
/// the order: nothing but this produces a [`ReferencedCandidate`], and nothing
/// but a [`ReferencedCandidate`] reaches the append.
///
/// # Errors
///
/// A missing or mismatched object, a ref already at another sha, or a Git error.
pub fn create_candidates_ref(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    promoting: PromotingCandidate,
) -> Result<ReferencedCandidate, UpstrokeError> {
    let PromotingCandidate {
        candidate,
        prepared_ref,
        base,
        tree,
    } = promoting;

    // "verify object".
    verify_object(manager, &candidate, &base, &tree)?;

    // "create exact candidates ref zero-old if absent".
    match manager.direct_ref_target(candidate.candidate_ref.as_str())? {
        None => manager.create_ref_zero_old(
            hooks.effects(),
            RefSite::CreateCandidates,
            candidate.candidate_ref.as_str(),
            candidate.commit_sha.as_str(),
        )?,
        Some(found) if found == candidate.commit_sha.0 => {}
        Some(found) => {
            return Err(Refusal::RefAtAnotherSha {
                refname: candidate.candidate_ref.0.clone(),
                found,
                expected: candidate.commit_sha.0.clone(),
            }
            .into());
        }
    }

    Ok(ReferencedCandidate {
        candidate,
        prepared_ref,
    })
}

/// **The append half.** `task_candidate_created`, and nothing else.
///
/// Skipped when the generation has already left `Promoting`, which is the one
/// durable fact that says this append landed: `apply_candidate_created` closes
/// the generation.
///
/// # Errors
///
/// Whatever the journal returns.
pub fn append_candidate_created(
    journal: &mut dyn CandidateJournal,
    referenced: ReferencedCandidate,
) -> Result<CreatedCandidate, UpstrokeError> {
    let ReferencedCandidate {
        candidate,
        prepared_ref,
    } = referenced;

    if is_promoting(journal.fold(), candidate.key, candidate.generation) {
        journal.emit(TopologyEventBody::TaskCandidateCreated {
            data: TaskCandidateCreated {
                candidate: candidate.clone(),
            },
        })?;
    }

    Ok(CreatedCandidate {
        candidate,
        prepared_ref,
    })
}

/// **The reclaim half, after the append.** Prune the pin, then scrub the
/// worktree and its intent.
///
/// `side_effect_vs_event_ordering`: "pin pruning and scrub (forced) after
/// task_candidate_created". The typestate is what enforces that here — this
/// takes a [`CreatedCandidate`] and only [`append_candidate_created`] makes
/// one.
///
/// # Errors
///
/// A Git or I/O error from the prune or the scrub.
pub fn reclaim_after_creation(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    worktree: &Slot,
    created: CreatedCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let CreatedCandidate {
        candidate,
        prepared_ref,
    } = created;

    // **"prune the pin" — expected-old against the *recorded* commit, not against
    // whatever is there.** This re-read the target and deleted that value, so a
    // pin substituted at any point before this line was removed with a
    // successful expected-old delete: the compare-and-swap compared the ref to
    // itself and could not fail. Deleting a substituted pin destroys the
    // evidence of the substitution, which is the opposite of what DESIGN §15
    // requires of an identity mismatch.
    //
    // The candidate this reclaim is for is `created.candidate`, whose
    // `commit_sha` came from the durable record. That is what the pin must
    // point at, and it is what the delete compares against.
    if let Some(found) = manager.direct_ref_target(prepared_ref.as_str())? {
        if found != candidate.commit_sha.0 {
            return Err(Refusal::RefAtAnotherSha {
                refname: prepared_ref.0.clone(),
                found,
                expected: candidate.commit_sha.0.clone(),
            }
            .into());
        }
        manager.delete_ref_expected_old(
            hooks.effects(),
            RefSite::DeleteCandidatePin,
            prepared_ref.as_str(),
            &found,
        )?;
    }

    // `cleanup`: the scrub, forced, and its intent with it.
    manager.remove_worktree(hooks.effects(), worktree)?;
    manager.remove_intent(hooks.effects(), worktree)?;

    Ok(QueuedCandidate { candidate })
}

/// Steps 3 to 6: everything after the settlement, for a caller with nothing to
/// interpose between them.
///
/// There is deliberately **no** function that runs steps 1 to 6. The
/// generation's settlement lands between step 2 and step 3 and is not this
/// module's — see the module documentation.
///
/// # Errors
///
/// Any error of [`append_candidate_prepared`] or [`complete_promotion`].
pub fn promote(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    journal: &mut dyn CandidateJournal,
    worktree: &Slot,
    pinned: PinnedCandidate,
) -> Result<QueuedCandidate, UpstrokeError> {
    let promoting = append_candidate_prepared(journal, pinned)?;
    complete_promotion(manager, hooks, journal, worktree, promoting)
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

/// A pin that no `candidate_prepared` names.
///
/// `transaction_fault_matrix[T-CAND-OBJ].resume_action` (b): "delete the exact
/// orphan pin expected-old, after which the object is again Git's". *Exact* is
/// why the object it points at travels with the name: an expected-old delete
/// against a value the coordinator never read is an unconditional delete
/// wearing a conditional API.
#[derive(Debug, PartialEq, Eq)]
pub struct OrphanPin {
    /// The pin.
    pub refname: GitRef,
    /// What it points at, read now.
    pub object: CommitSha,
}

/// What the candidate sequence still owes one generation.
///
/// A product of three independent answers rather than one of three cases, and
/// that is deliberate. `transaction_fault_matrix` states *boundaries* — T-CAND-OBJ
/// (a) and (b), T-CAND-REF — and a boundary is a place a run can be
/// interrupted, not a partition of every state a run can be found in. A sum
/// type over the boundaries would have to answer for the states between them
/// (a promotion whose append landed and whose pin prune did not; a pin left by
/// a resume that was itself killed), and every one of those would arrive as a
/// new variant. These three fields are total over all of them.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "what a resume owes is not discharged by classifying it"]
pub struct CandidateRecovery {
    /// `T-CAND-REF`: a promotion this generation has not finished.
    ///
    /// `Some` while `candidate_prepared` is durable and either
    /// `task_candidate_created` is not, or the pin it should have pruned is
    /// still on disk. [`complete_promotion`] is what discharges it and is
    /// idempotent, so both cases are the same answer rather than two.
    pub promotion: Option<PromotingCandidate>,

    /// `T-CAND-OBJ` (b): a pin no durable record accounts for.
    ///
    /// `Some` only while nothing durable names a candidate. Once
    /// `candidate_prepared` is durable the pin is *accounted for*, and
    /// [`complete_promotion`] prunes it in its place — a resume that pruned it
    /// as an orphan would drop the commit's only reference before its
    /// authoritative one exists.
    pub orphan_pin: Option<OrphanPin>,

    /// `T-CAND-OBJ`: the attempt is unsettled, so the resume settles it
    /// interrupted.
    ///
    /// **Reported, not performed.** The settlement is O24's and belongs to
    /// `settle.rs`; what this module knows is that the candidate sequence has
    /// nothing to promote, which is the reason the attempt is settled that way
    /// rather than by a success.
    pub settles_interrupted: bool,
}

impl CandidateRecovery {
    /// Nothing owed at all.
    pub const NOTHING: Self = Self {
        promotion: None,
        orphan_pin: None,
        settles_interrupted: false,
    };

    /// Nothing is owed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.promotion.is_none() && self.orphan_pin.is_none() && !self.settles_interrupted
    }
}

/// Classify what one generation owes, from the derived state and the refs on
/// disk.
///
/// The fold is the input because the fold *is* the derived state — INV-02's
/// "live state and replay use one checked transition over the exact wire
/// event". A classifier that re-read `events.jsonl` for itself would be a
/// second derivation, and the two would answer differently exactly once.
///
/// The **last** generation is the one classified, not the open one. A promotion
/// whose `task_candidate_created` landed has already closed its generation, and
/// a coordinator killed between that append and the pin's pruning leaves a pin
/// behind a generation that is `Closed` — which a classifier that only looked
/// at open generations would report as owing nothing, forever.
///
/// # Errors
///
/// A Git error reading the pin.
pub fn recovery_for(
    manager: &WorkspaceManager,
    run_id: &str,
    fold: &TopologyFold,
    key: TaskKey,
) -> Result<CandidateRecovery, UpstrokeError> {
    let Some(generation) = fold.task(key).and_then(|task| task.generations.last()) else {
        return Ok(CandidateRecovery::NOTHING);
    };
    let names = CandidateNames::of(run_id, key, generation.id);
    let pin = manager.direct_ref_target(names.prepared_ref.as_str())?;

    let Some(prepared) = generation.candidate.as_ref() else {
        // Nothing durable names a candidate, so the pin — if the run left one
        // — is an orphan, and an attempt that was running is settled
        // interrupted.
        return Ok(CandidateRecovery {
            promotion: None,
            orphan_pin: pin.map(|object| OrphanPin {
                refname: names.prepared_ref,
                object: CommitSha(object),
            }),
            settles_interrupted: matches!(generation.class, GenerationClass::InFlight { .. }),
        });
    };

    // **The pin binds to the record.** `candidate_prepared` is durable and names
    // a commit; a pin present at any other object is a substitution, and DESIGN
    // §15's extended exact-identity rule refuses it rather than proceeding —
    // "any substituted or symbolic pin … refuses while preserving evidence".
    //
    // This read the pin only as `pin.is_some()`, so a pin moved from the
    // recorded commit `C` to some `X` after `candidate_prepared` left the resume
    // promoting `C`, appending `task_candidate_created`, and then deleting the
    // substituted pin expected-old on the way out — succeeding, and erasing the
    // one ref that evidenced the substitution. The `bf927f3` review's second P1.
    //
    // Refused **here**, before any effect: this is a predicate over the durable
    // record and one ref read, and a refusal belongs before the first append.
    // Nested rather than a let-chain: MSRV is 1.85 and let-chains are 1.88.
    if let Some(found) = pin.as_deref() {
        if found != prepared.candidate.commit_sha.as_str() {
            return Err(Refusal::RefAtAnotherSha {
                refname: names.prepared_ref.0.clone(),
                found: found.to_owned(),
                expected: prepared.candidate.commit_sha.0.clone(),
            }
            .into());
        }
    }

    // The promotion is unfinished while the queue position is missing, and also
    // while the pin it should have pruned is still there. The candidate's
    // identity comes from the durable record rather than from this module's own
    // derivation of the names, because the record is what the run actually
    // wrote.
    let unfinished = generation.class == GenerationClass::Promoting || pin.is_some();
    Ok(CandidateRecovery {
        promotion: unfinished.then(|| PromotingCandidate {
            candidate: prepared.candidate.clone(),
            prepared_ref: names.prepared_ref,
            base: prepared.base_sha.clone(),
            tree: prepared.tree_sha.clone(),
        }),
        orphan_pin: None,
        settles_interrupted: false,
    })
}

/// Delete an orphan pin expected-old, after which its object is again Git's.
///
/// # Errors
///
/// A symbolic pin, an expected-old mismatch, or a Git error.
pub fn prune_orphan_pin(
    manager: &WorkspaceManager,
    hooks: &mut dyn TopologyHooks,
    pin: OrphanPin,
) -> Result<(), UpstrokeError> {
    manager.delete_ref_expected_old(
        hooks.effects(),
        RefSite::DeleteCandidatePin,
        pin.refname.as_str(),
        pin.object.as_str(),
    )
}

/// Every ref this run is entitled to have under its own namespace.
///
/// `expected_failures_refusals[2]` and
/// `transaction_fault_matrix[T-CAND-OBJ].refusal_condition` both refuse "an
/// unexpected ref under the run namespace", and
/// [`WorkspaceManager::refuse_unexpected_refs`] performs the refusal — but only
/// against a list somebody derived. This is that derivation, and it comes from
/// the fold rather than from a walk of the refs themselves, so a ref with no
/// durable record behind it is exactly what fails.
///
/// **The pin is expected for every generation; the candidates ref only for a
/// generation that prepared one.** The asymmetry is the durable order. A pin is
/// written *before* anything records it — that is the whole of T-CAND-OBJ (b) —
/// so a list that expected a pin only where a record already names one would
/// refuse the run at exactly the prefix whose recovery is to prune it, and the
/// refusal would fire before the pruning could. A candidates ref has the
/// opposite order: `candidate_prepared` is durable before it is created, so
/// requiring the record costs a resume nothing and refuses a ref no candidate
/// ever justified.
///
/// The integration ref is absent because it lives under `refs/heads/`, outside
/// this namespace.
#[must_use]
pub fn expected_refs(run_id: &str, fold: &TopologyFold) -> Vec<String> {
    let mut expected = Vec::new();
    let Some(registry) = fold.registry() else {
        return expected;
    };
    for entry in registry.entries() {
        let key = entry.key;
        let Some(task) = fold.task(key) else {
            continue;
        };
        for generation in &task.generations {
            let names = CandidateNames::of(run_id, key, generation.id);
            if generation.candidate.is_some() {
                // Created after `candidate_prepared` is durable, and never
                // pruned while the run can resume.
                expected.push(names.candidate_ref.0);
            }
            // Written before any record of it, and pruned after promotion or
            // as an orphan.
            expected.push(names.prepared_ref.0);
        }
    }
    expected
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Whether the generation that prepared `candidate` is still `Promoting`.
fn is_promoting(fold: &TopologyFold, key: TaskKey, generation: GenerationId) -> bool {
    fold.task(key).is_some_and(|task| {
        task.generations
            .iter()
            .any(|open| open.id == generation && open.class == GenerationClass::Promoting)
    })
}

/// `T-CAND-REF`'s "verify object", as the packet's own predicate.
///
/// `classify_object_residue` at `Object.CandidateCommitTree` with the recorded
/// id answers `After` exactly when that object is present — the site's after
/// phase is `AfterEffect::Unreferenced`, "the object is present and nothing
/// references it", and the classifier's `After` arm for the two commit-tree
/// sites is `object_exists` and nothing else. Reusing it rather than asking Git
/// again here keeps one answer to "is the candidate commit in this
/// repository?".
fn verify_object(
    manager: &WorkspaceManager,
    candidate: &CandidateRef,
    base: &CommitSha,
    tree: &CommitSha,
) -> Result<(), UpstrokeError> {
    let repository: &Path = manager.base();
    let residue = classify_object_residue(
        EffectSiteId::Object(ObjectSite::CandidateCommitTree),
        &ResidueTarget::new(repository).published(candidate.commit_sha.as_str()),
    )?;
    if residue != ObjectResidue::After {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }

    // **Existence is not identity.** DESIGN.md §15: `candidate_prepared`
    // records the complete attempt/base/commit/tree identity "so resume adopts
    // only the judged object". Presence alone accepts any object that happens
    // to be at that sha — an unrelated commit, or a blob — which is exactly what
    // a resume must not adopt.
    //
    // What is checkable here is what the fold keeps: the generation's base. A
    // candidate is a commit **on** that base, so an object that is not a commit
    // has no parent to read and one that is a different commit has the wrong
    // parent. Neither can pass.
    //
    let parent = manager.commit_parent(candidate.commit_sha.as_str())?;
    if parent.as_deref() != Some(base.as_str()) {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }

    // **And the tree, which is what was actually judged.** The parent says the
    // commit sits where the work started; it says nothing about the content.
    // A commit with the recorded parent and a *different* tree used to pass
    // here — so a resume could create the authoritative candidate ref at an
    // object no gate ran against and no reviewer read, which is the one thing
    // §15's "adopts only that exact shape" exists to prevent. This comment used
    // to record that gap and call closing it "its own decision"; the decision
    // was made on 2026-08-26 (Class B, `reviews/FINDINGS.md` §3) and
    // `PreparedCandidate::tree_sha` is it.
    //
    // Refused as `ObjectMissing` rather than a new refusal kind: the object at
    // that sha is not the candidate this run prepared, and "the candidate is
    // not here" is what that means for the caller. A new variant would be a
    // change to a refusal inventory this slice does not own.
    let found = manager.commit_tree_sha(candidate.commit_sha.as_str())?;
    if found.as_deref() != Some(tree.as_str()) {
        return Err(Refusal::ObjectMissing {
            key: candidate.key.0,
            generation: candidate.generation.0,
            commit: candidate.commit_sha.0.clone(),
        }
        .into());
    }
    Ok(())
}

fn refuse_malformed_commit(
    key: TaskKey,
    generation: GenerationId,
    commit: &CommitSha,
) -> Result<(), Refusal> {
    if is_object_id(commit.as_str()) {
        return Ok(());
    }
    Err(Refusal::MalformedCommit {
        key: key.0,
        generation: generation.0,
        value: commit.0.clone(),
    })
}

#[cfg(test)]
mod tests;
