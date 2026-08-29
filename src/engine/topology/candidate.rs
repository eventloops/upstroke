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
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::agent::ProcessOutput;
    use crate::events::log::{EventLog, TopologyLine, site_for};
    use crate::events::{BindingSummary, ChainSummary};
    use crate::events::{GateSummary, ReviewPassOutcome, ReviewRecord};
    use crate::gates::ShellKind;
    use crate::ir::{Effort, Plan, PlanSource, ResolvedEffortPolicy, Task, TaskId, TaskKind, Tier};
    use crate::review::{PassBinding, ReviewPlan};
    use crate::runner::container::view::fixtures as git_fixtures;
    use crate::runner::invocation::AttemptRole;
    use crate::runner::{CommandSpec, InvocationId, Runner, gate_request, host::HostRunner};
    use crate::topology::effects::{
        EventSite, HookHarness, HookPhase, Injection, InjectionMode, SnapshotSite, SubEffectPoint,
        WorktreeSite,
    };
    use crate::topology::events::{
        AttemptNumber, IncarnationId, LeaseGrant, RungBinding, TaskDispatched, TopologyEvent,
    };
    use crate::topology::events::{AttemptStarted4, RunStarted4, TopologyLimits};
    use crate::topology::fold::{FrozenInputs, TopologyFold};
    use crate::topology::paths::{GitPath, PathGrammar, PathPolicy, PathPolicyVersion};
    use crate::topology::registry::TaskRegistry;
    use crate::topology::schema::TOPOLOGY_SCHEMA;
    use crate::util::DurabilityLedger;
    use crate::workspace_manager::{
        EffectHooks, HarnessEffects, SnapshotInput, SnapshotName, unreachable_objects,
    };

    const RUN_ID: &str = "01KZCAND00000000000000000G";
    const INCARNATION: &str = "01KZCANDINC0000000000000AG";
    const ALPHA: TaskKey = TaskKey(0);
    const GENERATION: GenerationId = GenerationId(0);
    const FIXED_TS: &str = "2026-08-23T11:22:33Z";
    const NORMALIZED_DIGEST: &str =
        "sha256:7777777777777777777777777777777777777777777777777777777777777777";

    /// The env keys a kill child reads. Named rather than spelled twice: the
    /// parent sets them and the child reads them, and a typo in either half is
    /// a child that panics for the wrong reason and a parent that reads a
    /// directory nothing wrote.
    const ENV_BASE: &str = "UPSTROKE_TEST_CAND_BASE";
    const ENV_PRIVATE: &str = "UPSTROKE_TEST_CAND_PRIVATE";
    const ENV_SITE: &str = "UPSTROKE_TEST_CAND_SITE";

    // -----------------------------------------------------------------------
    // Fixtures
    //
    // Every effect here goes through the funnel that owns its site, in tests
    // as in production: `src/engine/topology/**` carries no module-level allow
    // of a governed lint, so this module may not name `std::fs`'s writers or
    // `std::process::Command` at all. Git runs through the process funnel
    // (`crate::runner::container::view::fixtures`, which is where the crate
    // already keeps that helper), directories are made by the run-directory
    // funnel, and everything under the execution root is `WorkspaceManager`'s.
    // -----------------------------------------------------------------------

    /// A scratch directory tree, made through the run-directory funnel.
    ///
    /// `rundir::create_public_dir` is `RunDir.CreatePublicDir` — the crate's
    /// directory-creation funnel — driven here with production's no-op
    /// observer so a fixture contributes nothing to the coverage evidence.
    fn make_dir(path: &Path) {
        crate::rundir::create_public_dir(path, &mut crate::rundir::NoHooks)
            .expect("the scratch directory");
    }

    /// Remove a fixture tree, through the funnel that owns tree removal.
    fn drop_dir(path: &Path) {
        let _ = crate::rundir::remove_public_husk(path, &mut crate::rundir::NoHooks);
    }

    /// A real repository, a real private root, a manager over both, and one
    /// task worktree carrying a staged edit.
    struct Fixture {
        root: PathBuf,
        base: PathBuf,
        private: PathBuf,
        manager: WorkspaceManager,
        /// The commit the task worktree was created at.
        base_sha: CommitSha,
        /// The task worktree's slot.
        task: Slot,
        /// The tree the worker produced, staged behind that worktree's index.
        tree_sha: CommitSha,
    }

    impl Fixture {
        /// Build the repository, the private root and the task worktree.
        ///
        /// `root` is created by the caller when it has to be predictable
        /// across processes, and by this function otherwise.
        fn at(root: PathBuf) -> Self {
            let base = root.join("repo");
            let private = root.join("private");
            make_dir(&private);
            let (head, _previous) = git_fixtures::repository(&base);

            let manager = WorkspaceManager::derive(&base, &private, RUN_ID, INCARNATION)
                .expect("derive the manager");
            manager
                .create_execution_root(&mut crate::workspace_manager::NoHooks)
                .expect("create the execution root");

            let task = Slot::Task {
                key: "alpha".to_owned(),
                generation: GENERATION.0,
            };
            manager
                .write_intent(&mut crate::workspace_manager::NoHooks, &task)
                .expect("the task intent");
            let worktree = manager
                .add_worktree(&mut crate::workspace_manager::NoHooks, &task, &head)
                .expect("the task worktree");

            // The worker's edit, written by `git` because this module may not
            // write a file itself. `hash-object -w --stdin` puts the blob in
            // the object store and `update-index --add --cacheinfo` puts it
            // behind *this worktree's* index, which is exactly the R9 state
            // `Object.CandidateStage` leaves.
            let blob = git_fixtures::git_ok(
                &worktree,
                &["hash-object", "-w", "--stdin", "--path", "worker.txt"],
            );
            git_fixtures::git_ok(
                &worktree,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("100644,{blob},worker.txt"),
                ],
            );
            let tree = manager
                .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &task)
                .expect("write-tree");

            Self {
                root,
                base,
                private,
                manager,
                base_sha: CommitSha(head),
                task,
                tree_sha: CommitSha(tree),
            }
        }

        /// A fixture under a fresh scratch root.
        fn new(tag: &str) -> Self {
            Self::at(git_fixtures::scratch(&format!("cand-{tag}")))
        }

        /// What a judged tree hands the sequence, for this fixture.
        fn judged(&self) -> JudgedTree {
            JudgedTree {
                key: ALPHA,
                generation: GENERATION,
                attempt: Box::new(attempt_record()),
                base_sha: self.base_sha.clone(),
                tree_sha: self.tree_sha.clone(),
                message: "alpha: the judged tree".to_owned(),
                actual_paths: region(),
                lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region() },
            }
        }

        /// A commit on the same base whose **tree is different**.
        ///
        /// The shape `verify_object`'s parent check cannot tell from the real
        /// candidate: same parent, same everything the fold used to keep, and
        /// content no gate ran against. Built by staging a second blob before
        /// writing the tree, so the difference is real rather than a relabelled
        /// sha.
        fn divergent_tree_commit(&self, hooks: &mut Hooks) -> (CommitSha, CommitSha) {
            // A second entry behind the same index. The blob's bytes do not
            // matter; the *path* is what moves the tree, and moving the tree is
            // the whole point.
            let worktree = self.manager.slot_path(&self.task);
            let blob = git_fixtures::git_ok(
                &worktree,
                &["hash-object", "-w", "--stdin", "--path", "smuggled.txt"],
            );
            let blob = blob.trim().to_owned();
            git_fixtures::git_ok(
                &worktree,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("100644,{blob},smuggled.txt"),
                ],
            );
            let tree = self
                .manager
                .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &self.task)
                .expect("write-tree");
            assert_ne!(
                tree, self.tree_sha.0,
                "the fixture wrote the same tree twice, so this commit is not divergent"
            );
            let judged = JudgedTree {
                message: "alpha: a tree nobody judged".to_owned(),
                tree_sha: CommitSha(tree.clone()),
                ..self.judged()
            };
            let commit = write_candidate_commit(&self.manager, hooks, RUN_ID, judged)
                .expect("commit-tree")
                .commit_sha()
                .clone();
            (commit, CommitSha(tree))
        }

        /// A second commit on the same base, differing only in its message.
        ///
        /// A *sibling*: `verify_object` now checks the parent, so a test that
        /// wants to reach a later refusal needs an object that passes the
        /// identity check without being the recorded candidate. The base itself
        /// no longer serves — its parent is not the base.
        fn sibling_commit(&self, hooks: &mut Hooks) -> CommitSha {
            let judged = JudgedTree {
                message: "alpha: a sibling of the judged tree".to_owned(),
                ..self.judged()
            };
            write_candidate_commit(&self.manager, hooks, RUN_ID, judged)
                .expect("commit-tree")
                .commit_sha()
                .clone()
        }

        /// Every unreachable object in the repository, per `git fsck`.
        fn unreachable(&self) -> Vec<String> {
            unreachable_objects(&self.base).expect("fsck")
        }

        /// Whether `object` is unreachable per `git fsck --unreachable`.
        fn is_unreachable(&self, object: &str) -> bool {
            self.unreachable().iter().any(|id| id == object)
        }

        /// Every unreachable object that is a commit.
        fn unreachable_commits(&self) -> Vec<String> {
            self.unreachable()
                .into_iter()
                .filter(|id| {
                    git_fixtures::git_ok(&self.base, &["cat-file", "-t", id]).trim() == "commit"
                })
                .collect()
        }

        /// Whether `object` is in this repository at all.
        fn object_present(&self, object: &str) -> bool {
            git_fixtures::git(
                &self.base,
                &["cat-file", "-e", &format!("{object}^{{commit}}")],
            )
            .code
                == Some(0)
        }

        /// Every ref under this run's namespace.
        fn run_refs(&self) -> Vec<(String, String)> {
            self.manager
                .refs_under(&run_namespace(RUN_ID))
                .expect("for-each-ref")
        }

        fn journal(&self, hooks: &Hooks) -> Journal {
            Journal::open(&self.private, self.base_sha.clone(), hooks)
        }

        /// The git administrative directory of the task worktree — where an
        /// interrupted command's `index.lock` lands.
        fn task_admin_dir(&self) -> PathBuf {
            self.manager
                .common_git_dir()
                .join("worktrees")
                .join("kalpha-g0")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            drop_dir(&self.root);
        }
    }

    // -----------------------------------------------------------------------
    // The journal: a real schema-4 log and the fold over it
    // -----------------------------------------------------------------------

    /// [`CandidateJournal`] over a real `events.jsonl` and a real fold.
    ///
    /// Not a recording double. The claims this module makes are about durable
    /// bytes and about what the fold refuses, so a journal that recorded
    /// intentions would leave both untested — and the "once" of
    /// `kill_after_candidate_prepared_appends_candidate_created_once` is
    /// literally a count of lines in a file.
    struct Journal {
        log: EventLog,
        fold: TopologyFold,
        path: PathBuf,
        hooks: TracedEvents,
    }

    impl Journal {
        /// A log carrying `run_started`, a dispatch and a started attempt —
        /// and **not** its settlement.
        ///
        /// That is `transaction_fault_matrix[T-CAND-OBJ]`'s durable state
        /// exactly ("attempt_started only"; "attempt unsettled"), which is the
        /// state the commit-tree and the pin happen in.
        /// The step after them is `candidate_prepared`, which is the
        /// settlement — there is no separate one to append first.
        fn open(private: &Path, base_sha: CommitSha, hooks: &Hooks) -> Self {
            let path = private.join("events.jsonl");
            let mut warnings = Vec::new();
            let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings)
                .expect("open the schema-4 log");
            assert!(warnings.is_empty(), "a fresh log warns about nothing");
            let mut journal = Self {
                log,
                fold: TopologyFold::new(inputs()),
                path,
                hooks: hooks.events.clone(),
            };
            journal
                .emit(TopologyEventBody::RunStarted {
                    data: Box::new(run_started(base_sha.clone())),
                })
                .expect("run_started");
            journal
                .emit(TopologyEventBody::TaskDispatched {
                    data: TaskDispatched {
                        key: ALPHA,
                        generation: GENERATION,
                        base_sha,
                        worktree_path: "tasks/kalpha-g0".to_owned(),
                        lease: LeaseGrant::Predicted { paths: region() },
                        source_candidate: None,
                    },
                })
                .expect("task_dispatched");
            let binding = journal.binding();
            journal
                .emit(TopologyEventBody::AttemptStarted {
                    data: AttemptStarted4 {
                        key: ALPHA,
                        generation: GENERATION,
                        attempt: AttemptNumber(1),
                        rung: 0,
                        binding,
                        pool: None,
                        resume_session: None,
                        materialization_observed: None,
                    },
                })
                .expect("attempt_started");
            journal
        }

        /// Reopen an existing log and replay it: `resume is
        /// replay-then-continue, and there is no second path`.
        fn resume(private: &Path, hooks: &Hooks) -> Self {
            let path = private.join("events.jsonl");
            let bytes = std::fs::read(&path).expect("the log the dead run left");
            let events = TopologyFold::parse_log(&bytes).expect("the log parses");
            let fold = TopologyFold::replay(inputs(), &events).expect("the log replays");
            let mut warnings = Vec::new();
            let log = EventLog::open(EventSite::OpenLog, &path, &mut warnings)
                .expect("reopen the schema-4 log");
            Self {
                log,
                fold,
                path,
                hooks: hooks.events.clone(),
            }
        }

        fn binding(&self) -> RungBinding {
            let registry = self.fold.registry().expect("the run has started");
            let entry = registry.get(ALPHA).expect("alpha is registered");
            let frozen = &entry.ladder.rungs[0];
            RungBinding::from_frozen(frozen, entry.ladder.effort.implementation_for(frozen.tier))
        }

        /// How many committed lines of `kind` the log carries.
        fn count(&self, kind: &str) -> usize {
            let bytes = std::fs::read(&self.path).expect("the log");
            TopologyFold::parse_log(&bytes)
                .expect("the log parses")
                .iter()
                .filter(|event| event.body.kind() == kind)
                .count()
        }

        fn generation_class(&self) -> Option<GenerationClass> {
            self.fold
                .task(ALPHA)
                .and_then(|task| task.generations.first())
                .map(|generation| generation.class.clone())
        }
    }

    impl CandidateJournal for Journal {
        fn emit(&mut self, body: TopologyEventBody) -> Result<(), UpstrokeError> {
            let event = TopologyEvent {
                ts: FIXED_TS.to_owned(),
                body,
            };
            let (line, checked) = TopologyLine::round_trip(&event)?;
            let delta =
                self.fold
                    .plan_transition(&checked)
                    .map_err(|error| UpstrokeError::Refused {
                        message: error.to_string(),
                    })?;
            self.log
                .append_topology_hooked(site_for(&checked.body), &line, &mut self.hooks)?;
            self.fold.apply_delta(delta);
            Ok(())
        }

        fn fold(&self) -> &TopologyFold {
            &self.fold
        }
    }

    // -----------------------------------------------------------------------
    // The hook bundle, with a phase this suite can arm
    // -----------------------------------------------------------------------

    /// The git funnels, recording into the shared [`HookHarness`] and
    /// answering an armed injection at a hook **phase**.
    ///
    /// [`HookHarness::arm`] takes a [`SubEffectPoint`] and `hook()` answers
    /// `Proceed` to `Before` and `After` unconditionally, so a phase can only
    /// be armed by a double. The recording still goes to the shared harness —
    /// a double that kept its own log would take every site it touched out of
    /// the coverage evidence.
    #[derive(Debug, Clone)]
    struct ArmedEffects {
        inner: HarnessEffects,
        armed: Vec<(EffectSiteId, HookPhase, Injection)>,
        trace: Trace,
    }

    impl EffectHooks for ArmedEffects {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            let answered = self.inner.phase(site, phase);
            self.trace.push(site, phase);
            self.armed
                .iter()
                .find(|(armed, at, _)| *armed == site && *at == phase)
                .map_or(answered, |(_, _, injection)| *injection)
        }

        fn durability_ledger(&self) -> DurabilityLedger {
            self.inner.durability_ledger()
        }
    }

    /// The order the funnels ran in, which is what an ordering clause is about.
    ///
    /// The shared [`HookHarness`] counts executions and does not keep their
    /// order — deliberately, because coverage is a set question. O28 to O31 are
    /// order questions, so this records the sequence beside it rather than
    /// instead of it.
    #[derive(Debug, Clone, Default)]
    struct Trace(Arc<Mutex<Vec<String>>>);

    impl Trace {
        fn push(&self, site: EffectSiteId, phase: HookPhase) {
            if phase != HookPhase::Before {
                return;
            }
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(site.to_string());
        }

        /// Forget everything recorded so far.
        ///
        /// The fixture's own appends — the dispatch and the attempt start —
        /// are not part of any clause O28 to O31 states, and an ordering
        /// assertion that carried them would be an assertion about the
        /// fixture's prologue.
        fn reset(&self) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }

        /// The recorded sequence, with everything not in `of_interest` dropped.
        ///
        /// Filtered rather than compared whole: a fixture that also creates an
        /// execution root and writes an intent runs those funnels too, and an
        /// assertion over the unfiltered list would be an assertion about the
        /// fixture.
        fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {
            let wanted: Vec<String> = of_interest.iter().map(ToString::to_string).collect();
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .filter(|seen| wanted.contains(seen))
                .cloned()
                .collect()
        }
    }

    /// The append funnel, tracing into the same order and recording into the
    /// same harness.
    ///
    /// Wrapped rather than reused directly because an ordering clause that
    /// mentions an append — O29, O30, O31 all do — cannot be checked from a
    /// trace that only sees the git funnels.
    #[derive(Debug, Clone)]
    struct TracedEvents {
        inner: crate::events::log::HarnessEventHooks,
        trace: Trace,
    }

    impl crate::events::log::EventHooks for TracedEvents {
        fn phase(&mut self, site: EventSite, phase: HookPhase) {
            self.inner.phase(site, phase);
            self.trace.push(EffectSiteId::Event(site), phase);
        }
    }

    /// The five-family bundle, with [`ArmedEffects`] in front of the git
    /// families and the shared harness behind all five.
    struct Hooks {
        effects: ArmedEffects,
        events: TracedEvents,
        rest: crate::engine::topology::seams::HarnessTopologyHooks,
        harness: Arc<Mutex<HookHarness>>,
        trace: Trace,
    }

    impl Hooks {
        fn new() -> Self {
            let harness = Arc::new(Mutex::new(HookHarness::new()));
            let trace = Trace::default();
            Self {
                effects: ArmedEffects {
                    inner: HarnessEffects::new(Arc::clone(&harness)),
                    armed: Vec::new(),
                    trace: trace.clone(),
                },
                events: TracedEvents {
                    inner: crate::events::log::HarnessEventHooks::new(Arc::clone(&harness)),
                    trace: trace.clone(),
                },
                rest: crate::engine::topology::seams::HarnessTopologyHooks::new(Arc::clone(
                    &harness,
                )),
                harness,
                trace,
            }
        }

        fn arm_phase(&mut self, site: EffectSiteId, phase: HookPhase, injection: Injection) {
            self.effects.armed.push((site, phase, injection));
        }

        fn arm_point(&mut self, site: EffectSiteId, point: SubEffectPoint, mode: InjectionMode) {
            self.harness
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .arm(site, point, mode)
                .expect("the site exposes that point in that mode");
        }

        fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
            self.harness
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .observed(site, phase)
        }
    }

    impl TopologyHooks for Hooks {
        fn effects(&mut self) -> &mut dyn EffectHooks {
            &mut self.effects
        }

        fn rundir(&mut self) -> &mut dyn crate::rundir::RunDirHooks {
            self.rest.rundir()
        }

        fn events(&mut self) -> &mut dyn crate::events::log::EventHooks {
            &mut self.events
        }

        fn container(&mut self) -> &mut dyn crate::runner::container::ContainerHooks {
            self.rest.container()
        }

        fn spawn(&mut self) -> &mut dyn crate::agent::proc::SpawnHooks {
            self.rest.spawn()
        }
    }

    // -----------------------------------------------------------------------
    // The frozen inputs of the fold
    // -----------------------------------------------------------------------

    fn region() -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath::from("worker.txt")],
        }
    }

    fn plan() -> Plan {
        Plan {
            source: PlanSource {
                adapter: "markdown".to_owned(),
                hash: "candidate-frozen-hash".to_owned(),
            },
            tasks: vec![Task {
                id: TaskId::from("alpha"),
                kind: TaskKind::Refactor,
                title: "  alpha — Ünicode title  ".to_owned(),
                body: "alpha body".to_owned(),
                depends_on: Vec::new(),
                acceptance: vec!["alpha holds".to_owned()],
                path_hints: vec!["worker.txt".to_owned()],
                suggested_tier: Some(Tier::Mid),
                min_tier: None,
                artifacts_in: Vec::new(),
                artifacts_out: Vec::new(),
            }],
            artifacts: Vec::new(),
        }
    }

    fn inputs() -> FrozenInputs {
        FrozenInputs {
            plan: plan(),
            normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        }
    }

    fn chain() -> ChainSummary {
        ChainSummary {
            task: "alpha".to_owned(),
            attempts_per: 1,
            bindings: Some(vec![BindingSummary {
                tier: Tier::Mid,
                agent: "alpha-agent".to_owned(),
                model: "alpha-model".to_owned(),
                pinned: false,
            }]),
            tiers: vec![Tier::Mid],
        }
    }

    fn run_started(base_sha: CommitSha) -> RunStarted4 {
        let started = RunStarted4 {
            schema: TOPOLOGY_SCHEMA,
            upstroke_version: "0.2.0-candidate".to_owned(),
            run_id: RUN_ID.to_owned(),
            incarnation: IncarnationId(INCARNATION.to_owned()),
            runner: crate::runner::policy::host_policy(),
            probed_agents: vec!["alpha-agent".to_owned()],
            branch: format!("upstroke/run-{RUN_ID}"),
            integration_ref: GitRef(format!("refs/heads/upstroke/run-{RUN_ID}")),
            base_sha,
            execution_root: "/var/lib/upstroke/candidate roots".to_owned(),
            private_dir: "/var/lib/upstroke/candidate private".to_owned(),
            plan_path: "docs/Candidate Plan.md".to_owned(),
            config_path: None,
            plan_hash: "candidate-frozen-hash".to_owned(),
            normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
            registry_digest: String::new(),
            path_policy: PathPolicy {
                version: PathPolicyVersion::V1,
                case_fold: true,
                grammar: PathGrammar::Globset,
            },
            limits: TopologyLimits {
                max_parallel: 3,
                max_defers: 2,
                max_merge_repairs: 1,
            },
            gates: vec!["fmt".to_owned()],
            gates_from_config: false,
            gate_cmds: vec![GateSummary {
                name: "fmt".to_owned(),
                cmd: "cargo fmt --check".to_owned(),
                timeout: Duration::from_secs(451),
                shell: ShellKind::Bash,
            }],
            interaction_mode: "never".to_owned(),
            chains: vec![chain()],
            effort_policy: ResolvedEffortPolicy {
                small: Effort::Low,
                mid: Effort::High,
                frontier: Effort::Max,
                review: Effort::Medium,
            },
            reviews: ReviewPlan {
                // Enabled: this fixture's `candidate_prepared` records carry a
                // passed `review` pass, and a run that froze verification off
                // obliges none — a combination `plan_for` cannot produce, since
                // its disabled branch resolves no `primary` either.
                enabled: Some(true),
                alternative_available: Some(false),
                pass_timeout_secs: Some(97),
                primary: Some(PassBinding::new("alpha-agent", "alpha-model")),
                alternative: None,
                second_opinion: vec![None],
            },
        };
        let digest = TaskRegistry::originals_with_agents(
            &plan(),
            &started.registry_record(),
            &started.probed_agents,
        )
        .expect("the fixture derives a registry")
        .digest();
        RunStarted4 {
            registry_digest: digest,
            ..started
        }
    }

    // =======================================================================
    // T-CAND-OBJ — the object, the pin, and what a resume does with neither
    // =======================================================================

    /// The child of the three T-CAND-OBJ kill tests.
    ///
    /// One child for three prefixes, because the setup up to the kill is the
    /// same in all three and a second child is a second thing to keep in step
    /// with this module.
    ///
    /// `Injection::Kill` is `std::process::abort()` — a real process death,
    /// chosen because the claim is *what a coordinator that runs no cleanup
    /// leaves on disk*, and an early `return` would unwind and prove something
    /// weaker. The `unreachable!` below is what fails the test if an injection
    /// silently stops killing.
    #[test]
    #[ignore = "spawned as a subprocess by the T-CAND-OBJ kill tests"]
    fn candidate_kill_child() {
        let base = PathBuf::from(std::env::var(ENV_BASE).expect("the base"));
        let private = PathBuf::from(std::env::var(ENV_PRIVATE).expect("the private root"));
        let which = std::env::var(ENV_SITE).expect("the site");

        let manager = WorkspaceManager::derive(&base, &private, RUN_ID, INCARNATION)
            .expect("the child derives the same manager");
        let task = Slot::Task {
            key: "alpha".to_owned(),
            generation: GENERATION.0,
        };
        let tree = manager
            .candidate_write_tree(&mut crate::workspace_manager::NoHooks, &task)
            .expect("the tree the parent staged");
        let head = git_fixtures::git_ok(&base, &["rev-parse", "HEAD"]);
        let judged = JudgedTree {
            key: ALPHA,
            generation: GENERATION,
            attempt: Box::new(attempt_record()),
            base_sha: CommitSha(head),
            tree_sha: CommitSha(tree),
            message: "alpha: the judged tree".to_owned(),
            actual_paths: region(),
            lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths: region() },
        };

        let mut hooks = Hooks::new();
        let pin = EffectSiteId::Ref(RefSite::PinCandidatePrepared);
        match which.as_str() {
            "id-unread" => {
                hooks.arm_point(
                    EffectSiteId::Object(ObjectSite::CandidateCommitTree),
                    SubEffectPoint::IdUnread,
                    InjectionMode::Kill,
                );
                let _ = write_candidate_commit(&manager, &mut hooks, RUN_ID, judged);
            }
            "before-pin" => {
                let unpinned = write_candidate_commit(&manager, &mut hooks, RUN_ID, judged)
                    .expect("commit-tree");
                hooks.arm_phase(pin, HookPhase::Before, Injection::Kill);
                let _ = pin_candidate(&manager, &mut hooks, unpinned);
            }
            "after-pin" => {
                let unpinned = write_candidate_commit(&manager, &mut hooks, RUN_ID, judged)
                    .expect("commit-tree");
                hooks.arm_phase(pin, HookPhase::After, Injection::Kill);
                let _ = pin_candidate(&manager, &mut hooks, unpinned);
            }
            // T-CAND-REF: `candidate_prepared` durable, and the coordinator
            // dies before the candidates ref exists. The whole point of this
            // prefix is that the parent then resumes from the child's own
            // durable log.
            "after-prepared" => {
                let mut journal = Journal::open(&private, judged.base_sha.clone(), &hooks);
                let unpinned = write_candidate_commit(&manager, &mut hooks, RUN_ID, judged)
                    .expect("commit-tree");
                let pinned = pin_candidate(&manager, &mut hooks, unpinned).expect("pin");
                let promoting =
                    append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");
                hooks.arm_phase(
                    EffectSiteId::Ref(RefSite::CreateCandidates),
                    HookPhase::Before,
                    Injection::Kill,
                );
                let _ = complete_promotion(&manager, &mut hooks, &mut journal, &task, promoting);
            }
            other => panic!("unknown site `{other}`"),
        }
        unreachable!("the kill must have taken this process");
    }

    /// Run [`candidate_kill_child`] against `fixture` at `which`, and hand back
    /// what the dead child's process left.
    ///
    /// The spawn goes through the process funnel — `Process.Spawn` is the site
    /// that owns starting a process, in a test as in production.
    fn spawn_kill_child(fixture: &Fixture, which: &str) -> ProcessOutput {
        let exe = std::env::current_exe().expect("the test binary");
        let spec = CommandSpec::new(exe.to_string_lossy().into_owned())
            .arg("--exact")
            .arg("engine::topology::candidate::tests::candidate_kill_child")
            .arg("--ignored")
            .arg("--nocapture")
            .env(ENV_BASE, fixture.base.to_string_lossy().into_owned())
            .env(ENV_PRIVATE, fixture.private.to_string_lossy().into_owned())
            .env(ENV_SITE, which);
        HostRunner::new()
            .run(&gate_request(
                spec,
                fixture.root.clone(),
                Duration::from_secs(120),
                InvocationId::attempt(ALPHA, GENERATION, AttemptNumber(1), AttemptRole::Gate(0), 0),
            ))
            .expect("the child runs through the process funnel")
    }

    /// The child died where it was armed rather than panicking somewhere else.
    ///
    /// `!success` alone does not say that: a child whose injection stopped
    /// firing reaches `unreachable!`, panics, and exits non-zero too. The panic
    /// message on stderr is what tells the two apart.
    fn assert_killed(output: &ProcessOutput, which: &str) {
        assert!(
            !output.stderr.contains("panicked at"),
            "`{which}`: the child panicked instead of being killed: {}",
            output.stderr
        );
        assert_ne!(
            output.code,
            Some(0),
            "`{which}`: the child must not have exited cleanly"
        );
    }

    /// The commit `git cat-file` shows at `object`, as its raw header.
    fn commit_body(fixture: &Fixture, object: &str) -> String {
        git_fixtures::git_ok(&fixture.base, &["cat-file", "commit", object])
    }

    /// T-CAND-OBJ (a), reached by killing between the commit-tree funnel's
    /// return and the pin.
    ///
    /// `durable_state` is "attempt_started only" and `authoritative_state` is
    /// "attempt unsettled; the object is Git/GC-owned residue", so this asserts
    /// three things: the object is **present**, it is **unreachable** per
    /// `git fsck --unreachable` (R27 is a claim about reachability, not about
    /// deletion), and the resume settles the attempt interrupted with nothing
    /// to delete.
    #[test]
    fn kill_after_commit_tree_before_pin_leaves_gc_owned_object_and_settles_interrupted() {
        let fixture = Fixture::new("kill-before-pin");
        assert!(
            fixture.unreachable_commits().is_empty(),
            "the fixture starts with no unreachable commit, so the one below is the child's"
        );

        let output = spawn_kill_child(&fixture, "before-pin");
        assert_killed(&output, "before-pin");

        let orphans = fixture.unreachable_commits();
        assert_eq!(
            orphans.len(),
            1,
            "the child wrote exactly one candidate commit and nothing referenced it: {orphans:?}"
        );
        let object = &orphans[0];
        assert!(
            fixture.object_present(object),
            "\"left to Git\" is not \"deleted\": the object is still in the store"
        );
        let body = commit_body(&fixture, object);
        assert!(
            body.contains(&format!("tree {}", fixture.tree_sha)),
            "it is the commit of the judged tree: {body}"
        );
        assert!(
            body.contains(&format!("parent {}", fixture.base_sha)),
            "parented on the base the work started from: {body}"
        );
        assert!(
            fixture.run_refs().is_empty(),
            "no pin exists: {:?}",
            fixture.run_refs()
        );

        // The resume: `settle attempt interrupted`, and nothing to delete.
        let journal = fixture.journal(&Hooks::new());
        let recovery =
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
        assert!(
            recovery.settles_interrupted,
            "the attempt is unsettled, so the resume settles it interrupted: {recovery:?}"
        );
        assert_eq!(
            recovery.orphan_pin, None,
            "an unpinned object leaves nothing for the resume to reclaim"
        );
        assert_eq!(
            recovery.promotion, None,
            "and nothing durable names a candidate, so nothing is promoted"
        );
    }

    /// T-CAND-OBJ (a) again, at the coordinate the packet names: the
    /// parent-executed `IdUnread` point.
    ///
    /// "the parent-executed IdUnread point lies between the child's exit and
    /// the coordinator recording the id". So the durable outcome is the same
    /// unreferenced object — and the *coordinator* never learned its id, which
    /// is why this test identifies the commit by its content rather than by a
    /// value anything recorded.
    ///
    /// `IdUnread` supports `Kill` and nothing else
    /// (`SubEffectPoint::modes`), so there is no error-return sibling to this
    /// test and inventing one would invent a resume action nothing tables.
    #[test]
    fn kill_at_commit_tree_id_unread_point_leaves_gc_owned_object() {
        assert_eq!(
            SubEffectPoint::IdUnread.modes(),
            &[InjectionMode::Kill],
            "an error-return sibling to this test would need a contract the packet does not give"
        );

        let fixture = Fixture::new("kill-id-unread");
        assert!(fixture.unreachable_commits().is_empty(), "clean start");

        let output = spawn_kill_child(&fixture, "id-unread");
        assert_killed(&output, "id-unread");

        let orphans = fixture.unreachable_commits();
        assert_eq!(
            orphans.len(),
            1,
            "commit-tree writes its object by temp-file-and-rename, so the object is whole \
             even though the id was never read: {orphans:?}"
        );
        let body = commit_body(&fixture, &orphans[0]);
        assert!(
            body.contains(&format!("tree {}", fixture.tree_sha))
                && body.contains(&format!("parent {}", fixture.base_sha)),
            "{body}"
        );
        assert!(
            fixture.run_refs().is_empty(),
            "nothing names it: {:?}",
            fixture.run_refs()
        );
    }

    /// T-CAND-OBJ (b): the pin exists and `candidate_prepared` does not, so the
    /// resume deletes the exact orphan pin expected-old and the object is again
    /// Git's.
    #[test]
    fn orphan_candidate_pin_removed_after_kill() {
        let fixture = Fixture::new("kill-after-pin");
        let output = spawn_kill_child(&fixture, "after-pin");
        assert_killed(&output, "after-pin");

        let refs = fixture.run_refs();
        assert_eq!(refs.len(), 1, "the pin, and only the pin: {refs:?}");
        let (refname, object) = &refs[0];
        assert_eq!(
            refname,
            candidate_pin_ref(RUN_ID, ALPHA, GENERATION).as_str(),
            "the pin is at the name the sequence derives"
        );
        assert!(
            !fixture.is_unreachable(object),
            "while the pin holds it the commit is R23, not R27"
        );

        let mut hooks = Hooks::new();
        let journal = fixture.journal(&hooks);
        let recovery =
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
        assert!(recovery.settles_interrupted && recovery.promotion.is_none());
        let Some(pin) = recovery.orphan_pin else {
            panic!("an unsettled attempt with a pin on disk owes an orphan pin");
        };
        assert_eq!(pin.refname.as_str(), refname);
        assert_eq!(&pin.object.0, object, "the pin's *exact* recorded value");

        prune_orphan_pin(&fixture.manager, &mut hooks, pin).expect("prune the orphan");
        assert!(
            fixture.run_refs().is_empty(),
            "the pin is gone: {:?}",
            fixture.run_refs()
        );
        assert!(
            fixture.is_unreachable(object),
            "\"after which the object is again Git's\""
        );
        assert!(fixture.object_present(object), "again Git's is not deleted");
        assert!(
            hooks.observed(
                EffectSiteId::Ref(RefSite::DeleteCandidatePin),
                HookPhase::Before
            ),
            "the deletion went through its own funnel site"
        );
    }

    /// `resume_action` (a) is "nothing to delete: the unpinned object is left
    /// to Git (**never adopted**; decision:295)".
    ///
    /// Adoption would be any of three things: a pin, a candidates ref, or a
    /// `candidate_prepared` naming the commit. None of them happens, and the
    /// object stays exactly where the interrupted run left it — present and
    /// unreachable — through a full recovery.
    #[test]
    fn unpinned_object_never_adopted_on_resume() {
        let fixture = Fixture::new("never-adopted");
        let mut hooks = Hooks::new();
        let journal = fixture.journal(&hooks);

        let unpinned =
            write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
                .expect("commit-tree");
        let object = unpinned.commit_sha().0.clone();
        // The witness is dropped without being pinned: the tabled outcome, not
        // a leak. Nothing else in this module can consume it.
        drop(unpinned);

        assert!(fixture.object_present(&object));
        assert!(fixture.is_unreachable(&object));

        let recovery =
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
        assert_eq!(
            recovery,
            CandidateRecovery {
                promotion: None,
                orphan_pin: None,
                settles_interrupted: true,
            },
            "the recovery names no object, so no later step has one to adopt"
        );

        // And the run namespace is entitled to no *candidates* ref: nothing
        // durable names a candidate, so a ref that appeared for one would be
        // exactly the unexpected-ref refusal.
        assert_eq!(
            expected_refs(RUN_ID, journal.fold()),
            vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
            "the pin alone, because a pin is written before anything records it"
        );
        fixture
            .manager
            .refuse_unexpected_refs(
                &run_namespace(RUN_ID),
                &expected_refs(RUN_ID, journal.fold()),
            )
            .expect("the namespace is empty, which is what it should be");

        assert!(
            fixture.object_present(&object),
            "still present after the recovery"
        );
        assert!(
            fixture.is_unreachable(&object),
            "still unreachable after the recovery: never adopted"
        );
    }

    // =======================================================================
    // T-CAND-REF — the authoritative ref, the queue position, and the pin
    // =======================================================================

    /// The whole sequence from the pin onwards, on a live run.
    ///
    /// Returns the fixture's judged candidate plus the hooks and journal it
    /// ran through, so a test can assert on the order the funnels ran in and on
    /// what the log holds.
    fn run_to_queued(fixture: &Fixture, hooks: &mut Hooks, journal: &mut Journal) -> CommitSha {
        hooks.trace.reset();
        let unpinned = write_candidate_commit(&fixture.manager, hooks, RUN_ID, fixture.judged())
            .expect("commit-tree");
        let commit = unpinned.commit_sha().clone();
        let pinned = pin_candidate(&fixture.manager, hooks, unpinned).expect("pin");
        promote(&fixture.manager, hooks, journal, &fixture.task, pinned).expect("promote");
        commit
    }

    /// **A substituted prepared pin is refused, and the evidence survives the
    /// refusal.**
    ///
    /// `DESIGN.md` §15's extended exact-identity rule: *"Any substituted or
    /// symbolic pin, third branch SHA, changed branch identity, or mismatched
    /// commit object refuses while preserving evidence."*
    ///
    /// `recovery_for` read the pin as `pin.is_some()` and never compared its
    /// target to the commit `candidate_prepared` recorded, and
    /// `reclaim_after_creation` re-read the target and deleted **that** value
    /// expected-old — a compare-and-swap comparing the ref to itself, which
    /// cannot fail. So a pin moved from the recorded `C` to some `X` after the
    /// settlement left a resume promoting `C`, appending
    /// `task_candidate_created`, and then removing the substituted pin on the
    /// way out: it succeeded, and it deleted the one ref that evidenced the
    /// substitution. The `bf927f3` review's second P1.
    ///
    /// Three claims, because "refuses while preserving evidence" is three
    /// things: it refuses, it names both shas so the substitution is legible
    /// from the error alone, and **nothing was appended, created or deleted** —
    /// the pin is still at the substituted object for a person to look at.
    #[test]
    fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {
        let fixture = Fixture::new("pin-substituted");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        // Reach the boundary honestly: commit, pin, `candidate_prepared`.
        let unpinned =
            write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
                .expect("commit-tree");
        let recorded = unpinned.commit_sha().clone();
        let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
        let promoting =
            append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");
        assert_eq!(journal.count("candidate_prepared"), 1);
        assert_eq!(journal.count("task_candidate_created"), 0);

        // The substitution: the pin is moved to a real sibling commit on the
        // same base. A different *tree* is not needed — the point is that the
        // pin no longer names the recorded commit, and this must be caught by
        // the pin's own binding rather than by the tree check.
        let impostor = fixture.sibling_commit(&mut hooks);
        assert_ne!(impostor, recorded);
        let names = CandidateNames::of(RUN_ID, ALPHA, GENERATION);
        git_fixtures::git_ok(
            &fixture.base,
            &["update-ref", names.prepared_ref.as_str(), impostor.as_str()],
        );

        // (1) Recovery refuses, before any effect.
        let refused = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
            .expect_err("a pin that is not the recorded commit is a substitution");

        // (2) The refusal names both, so the substitution is legible from it.
        let text = refused.to_string();
        assert!(
            text.contains(impostor.as_str()) && text.contains(recorded.as_str()),
            "the refusal must name what the pin points at and what the record says: {text}"
        );

        // (3) The evidence is intact: the pin is still at the impostor, the
        //     candidates ref was never created, and nothing was appended.
        assert_eq!(
            fixture
                .manager
                .direct_ref_target(names.prepared_ref.as_str())
                .expect("read the pin")
                .as_deref(),
            Some(impostor.as_str()),
            "the refusal removed or moved the pin, which is the evidence"
        );
        assert!(
            fixture
                .manager
                .direct_ref_target(names.candidate_ref.as_str())
                .expect("read the candidates ref")
                .is_none(),
            "a refused promotion created the authoritative candidate ref"
        );
        assert_eq!(journal.count("task_candidate_created"), 0);

        // And the reclaim half refuses the same substitution rather than
        // deleting it, for a caller that reached it another way.
        let referenced = create_candidates_ref(&fixture.manager, &mut hooks, promoting)
            .expect("the recorded commit still verifies");
        let created =
            append_candidate_created(&mut journal, referenced).expect("task_candidate_created");
        let refused = reclaim_after_creation(&fixture.manager, &mut hooks, &fixture.task, created)
            .expect_err("the prune compares against the recorded commit");
        assert!(
            refused.to_string().contains(impostor.as_str()),
            "the prune's refusal must name the substituted target: {refused}"
        );
        assert_eq!(
            fixture
                .manager
                .direct_ref_target(names.prepared_ref.as_str())
                .expect("read the pin")
                .as_deref(),
            Some(impostor.as_str()),
            "the prune deleted the substituted pin, which is the evidence"
        );
    }

    /// T-CAND-REF's boundary reached by a real kill, then its `resume_action`
    /// — and `task_candidate_created` lands **once**, however many times the
    /// closure procedure runs.
    ///
    /// The "once" is a count of committed lines in the log the dead process
    /// wrote, not a count of calls: the fold refuses a second
    /// `task_candidate_created` for a generation it has already closed, and a
    /// closure procedure that did not read that would append a line the fold
    /// then refuses on the next replay — a log that cannot be resumed.
    #[test]
    fn kill_after_candidate_prepared_appends_candidate_created_once() {
        let fixture = Fixture::new("kill-after-prepared");
        let output = spawn_kill_child(&fixture, "after-prepared");
        assert_killed(&output, "after-prepared");

        // The durable state of the boundary: the prepare landed, the queue
        // position did not, the pin is still holding the commit.
        let mut hooks = Hooks::new();
        let mut journal = Journal::resume(&fixture.private, &hooks);
        assert_eq!(journal.count("candidate_prepared"), 1);
        assert_eq!(journal.count("task_candidate_created"), 0);
        assert_eq!(journal.generation_class(), Some(GenerationClass::Promoting));
        let refs = fixture.run_refs();
        assert_eq!(
            refs.iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>(),
            vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
            "the pin, and no candidates ref"
        );

        let recovery =
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
        assert!(!recovery.settles_interrupted && recovery.orphan_pin.is_none());
        let Some(promoting) = recovery.promotion else {
            panic!("a durable candidate_prepared is an unfinished promotion");
        };
        // **The tree came off the fold, and it is the tree the event recorded.**
        // `promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged`
        // builds its `PromotingCandidate` by hand, so it proves the *check* and
        // not the *value it checks against*: the fold retaining `base_sha` in
        // that field left it green.
        //
        // **This is the resumed value, and it is the one that needed a witness.**
        // Production builds a `PromotingCandidate` in two places: `promote`
        // returns one carrying `judged.tree_sha` — the same value it has just
        // written into the event, so the comparison there cannot fail and
        // witnesses nothing — and `recovery_for` builds one from the fold,
        // where the number has been through a serialization, a replay and an
        // `apply`. Only the second can be wrong, and only the second is
        // asserted here.
        assert_eq!(
            promoting.tree, fixture.tree_sha,
            "the recovered promotion carries {} where `candidate_prepared` recorded \
             {}, so adoption would verify against the wrong number and a divergent \
             tree would pass",
            promoting.tree.0, fixture.tree_sha.0
        );
        let commit = promoting.candidate().commit_sha.clone();
        complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            promoting,
        )
        .expect("the closure procedure finishes it");
        assert_eq!(journal.count("task_candidate_created"), 1);

        // "the closure procedure performs the same steps at any run end": run
        // it again. Every step reads the world first, so the second run appends
        // nothing, refuses nothing, and leaves the same refs.
        assert!(
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
                .expect("classify again")
                .is_empty(),
            "a completed promotion — queue position appended, pin pruned — owes nothing"
        );
        let again = PromotingCandidate {
            candidate: CandidateRef {
                key: ALPHA,
                generation: GENERATION,
                commit_sha: commit.clone(),
                candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
            },
            prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
            base: fixture.base_sha.clone(),
            tree: fixture.tree_sha.clone(),
        };
        complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            again,
        )
        .expect("idempotent");
        assert_eq!(
            journal.count("task_candidate_created"),
            1,
            "twice through the closure procedure is still one queue position"
        );
        assert_eq!(
            fixture.run_refs(),
            vec![(
                candidates_ref(RUN_ID, ALPHA, GENERATION).0,
                commit.0.clone()
            )],
            "the candidates ref alone, at the recorded commit"
        );

        // And the refusal the same sentence names: a ref present at another
        // SHA is not accepted as "already created".
        let sibling = fixture.sibling_commit(&mut hooks);
        assert_ne!(sibling, commit, "a different commit on the same base");
        let forged = PromotingCandidate {
            candidate: CandidateRef {
                key: ALPHA,
                generation: GENERATION,
                commit_sha: sibling,
                candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
            },
            prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
            base: fixture.base_sha.clone(),
            tree: fixture.tree_sha.clone(),
        };
        let refused = complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            forged,
        )
        .expect_err("a candidates ref at another SHA refuses");
        assert!(
            refused.to_string().contains("is present at")
                && refused.to_string().contains(&commit.0),
            "the refusal names the ref's actual value: {refused}"
        );
    }

    /// O31, and `cleanup`'s "candidate-prepared pins pruned right after
    /// promotion" — with the order the clauses give, observed rather than
    /// assumed.
    ///
    /// The trace is the assertion. O28 puts the commit object before the pin,
    /// O29 the pin before `candidate_prepared`, O30 the candidates ref between
    /// the two appends, and O31 the pin's pruning and the scrub after
    /// `task_candidate_created` — which is one total order over six funnel
    /// sites, and this is it.
    #[test]
    fn pin_pruned_after_promotion() {
        let fixture = Fixture::new("pin-pruned");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);
        let commit = run_to_queued(&fixture, &mut hooks, &mut journal);

        assert_eq!(
            hooks.trace.order(&[
                EffectSiteId::Object(ObjectSite::CandidateCommitTree),
                EffectSiteId::Ref(RefSite::PinCandidatePrepared),
                EffectSiteId::Event(EventSite::Append),
                EffectSiteId::Ref(RefSite::CreateCandidates),
                EffectSiteId::Ref(RefSite::DeleteCandidatePin),
                EffectSiteId::Worktree(WorktreeSite::Remove),
            ]),
            vec![
                "Object.CandidateCommitTree".to_owned(),
                "Ref.PinCandidatePrepared".to_owned(),
                // **candidate_prepared — the settlement itself, and there is
                // one.** This list carried a second `Event.Append` above this
                // one for an `attempt_finished(succeeded)` between the pin and
                // the prepare, annotated "the settlement, which is not this
                // module's". It was not the settlement and it should not have
                // been appended: `candidate_prepared` is the sole successful
                // settlement for a candidate-producing attempt, per
                // `decisions/2026-08-12-merge-queue-execution-topology.md`, and
                // the 2026-08-27 ruling conformed the code to it. **Three
                // appends in this sequence, not four**, and the count is the
                // assertion — a build that re-introduced the pair would put the
                // fourth back and fail here.
                "Event.Append".to_owned(),
                "Ref.CreateCandidates".to_owned(),
                // task_candidate_created.
                "Event.Append".to_owned(),
                "Ref.DeleteCandidatePin".to_owned(),
                "Worktree.Remove".to_owned(),
            ],
            "O28 to O31, as one observed order — three appends, because \
             `candidate_prepared` is the sole successful settlement"
        );

        // What is left: the authoritative ref, and nothing else.
        assert_eq!(
            fixture.run_refs(),
            vec![(
                candidates_ref(RUN_ID, ALPHA, GENERATION).0,
                commit.0.clone()
            )],
            "the pin is pruned and the candidates ref is not"
        );
        assert!(
            !fixture.is_unreachable(&commit.0),
            "the candidates ref (R11) is what accounts for the commit now"
        );

        // `cleanup`: "candidates refs (R11) never pruned while the run can
        // resume". Nothing in this module deletes one — the site exists for
        // Complete finalization, and this sequence never reaches it.
        assert!(
            !hooks.observed(
                EffectSiteId::Ref(RefSite::DeleteCandidatesRef),
                HookPhase::Before
            ),
            "promotion pruned the authoritative ref"
        );
        // …and the expected-ref derivation says the same thing from the fold:
        // the candidates ref is expected, the pin no longer is.
        assert_eq!(
            expected_refs(RUN_ID, journal.fold()),
            vec![
                candidates_ref(RUN_ID, ALPHA, GENERATION).0,
                candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0,
            ],
            "a prepared candidate accounts for both names, whether or not either is on disk"
        );
        fixture
            .manager
            .refuse_unexpected_refs(
                &run_namespace(RUN_ID),
                &expected_refs(RUN_ID, journal.fold()),
            )
            .expect("the namespace carries exactly what the fold accounts for");
    }

    /// ST-17: "a Promoting generation is always promoted before any
    /// `run_finished`."
    ///
    /// Executed as the fold's own refusal rather than as a claim about this
    /// module: while the generation is `Promoting` the derived outcome is
    /// `NotEnding`, so `run_finished` cannot be appended at all; the closure
    /// procedure is what clears it.
    #[test]
    fn promoting_completed_at_run_end() {
        let fixture = Fixture::new("promoting-at-run-end");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        let unpinned =
            write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
                .expect("commit-tree");
        let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
        let promoting =
            append_candidate_prepared(&mut journal, pinned).expect("candidate_prepared");

        assert_eq!(journal.generation_class(), Some(GenerationClass::Promoting));
        assert_eq!(
            journal.fold().derived_outcome(),
            crate::topology::events::DerivedOutcome::NotEnding,
            "a Promoting generation blocks the run from ending"
        );
        let refused = journal
            .emit(TopologyEventBody::RunFinished {
                data: crate::topology::events::RunFinished4 {
                    outcome: crate::events::RunOutcome::Complete,
                    halted_at: None,
                    merged: 0,
                    parked: 0,
                },
            })
            .expect_err("run_finished before the promotion");
        assert!(
            refused.to_string().contains("not ending"),
            "the fold refuses it for the reason ST-17 gives: {refused}"
        );

        // The closure procedure, exactly as `resume_action` describes it.
        let Some(promoting_again) = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
            .expect("classify")
            .promotion
        else {
            panic!("a Promoting generation owes a promotion");
        };
        assert_eq!(
            promoting_again.candidate(),
            promoting.candidate(),
            "the closure procedure recovers the same candidate the live path holds"
        );
        complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            promoting,
        )
        .expect("promote at run end");

        assert_eq!(
            journal.generation_class(),
            Some(GenerationClass::Closed),
            "Promoting ends with `task_candidate_created`"
        );
        assert!(
            journal
                .fold()
                .task(ALPHA)
                .expect("alpha")
                .generations
                .iter()
                .all(|generation| generation.class != GenerationClass::Promoting),
            "no generation is left promoting"
        );
        assert_eq!(
            journal
                .fold()
                .queue()
                .expect("started")
                .entries()
                .iter()
                .map(|entry| entry.candidate.commit_sha.clone())
                .collect::<Vec<_>>(),
            vec![promoting_again.candidate().commit_sha.clone()],
            "and it holds its queue position"
        );
    }

    // =======================================================================
    // T-SCRUB — the forced, contained, idempotent reclaim
    // =======================================================================

    /// `resume_action`: "idempotent contained forced removal of the worktree
    /// and intent".
    ///
    /// Idempotent both ways round: the promotion already scrubbed, and a resume
    /// that re-runs the closure procedure scrubs again. Also `cleanup`'s "task
    /// worktree scrubbed **only after** `task_candidate_created` is durable" —
    /// asserted as an order, because a scrub that ran first would leave a
    /// promotion with no worktree to verify against.
    #[test]
    fn worktree_removal_idempotent_after_candidate_created() {
        let fixture = Fixture::new("scrub-idempotent");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);
        let path = fixture.manager.slot_path(&fixture.task);
        assert!(
            path.is_dir(),
            "the task worktree exists before the promotion"
        );

        run_to_queued(&fixture, &mut hooks, &mut journal);

        assert_eq!(journal.count("task_candidate_created"), 1);
        assert!(!path.exists(), "the worktree is scrubbed");
        assert!(
            !fixture.manager.intent_path(&fixture.task).exists(),
            "and its intent leaves with it"
        );
        assert!(
            !fixture.task_admin_dir().exists(),
            "and the row Git registered for it"
        );

        // The order `cleanup` states, from the trace: the append first.
        let order = hooks.trace.order(&[
            EffectSiteId::Event(EventSite::Append),
            EffectSiteId::Worktree(WorktreeSite::Remove),
        ]);
        assert_eq!(
            order.last().map(String::as_str),
            Some("Worktree.Remove"),
            "the scrub is after every append of the sequence: {order:?}"
        );

        // Again, and again through the funnel rather than by inspection.
        for round in 1..=2 {
            fixture
                .manager
                .remove_worktree(hooks.effects(), &fixture.task)
                .unwrap_or_else(|error| panic!("round {round}: {error}"));
            fixture
                .manager
                .remove_intent(hooks.effects(), &fixture.task)
                .unwrap_or_else(|error| panic!("round {round}: {error}"));
            assert!(!path.exists(), "round {round}");
        }

        // Containment, which is what makes an idempotent forced removal safe:
        // `refusal_condition` is "path outside execution root". Executed as a
        // refusal rather than asserted as a property of the happy path, because
        // an idempotent removal that had stopped checking would still pass
        // every assertion above.
        let escaping = Slot::Task {
            key: "../../escape".to_owned(),
            generation: GENERATION.0,
        };
        let refused = fixture
            .manager
            .remove_worktree(hooks.effects(), &escaping)
            .expect_err("a slot whose name leaves the execution root refuses");
        assert!(
            refused.to_string().contains("escape"),
            "the refusal names what it refused: {refused}"
        );
    }

    /// `cleanup`: removal is forced "so Git administrative residue left by an
    /// interrupted command (**index.lock**, …) never blocks reclaim — such
    /// residue belongs to the worktree's row and leaves with it".
    ///
    /// The control half is the first assertion. An `index.lock` that did not
    /// actually block anything would make the rest of this test pass against a
    /// removal that was never forced, which is the shape of the Windows
    /// `File::open` test this project has already paid for.
    #[test]
    fn worktree_removal_succeeds_with_index_lock_present() {
        let fixture = Fixture::new("scrub-index-lock");
        let admin = fixture.task_admin_dir();
        let lock = admin.join("index.lock");
        assert!(admin.is_dir(), "the worktree's row: {}", admin.display());

        // Planted by `git config --file`, because this module may not write a
        // file itself. What the residue *is* is a file at that name: nothing in
        // the removal path reads its bytes, and Git's own index writer refuses
        // on its existence alone — which the control below executes.
        git_fixtures::git_ok(
            &fixture.base,
            &[
                "config",
                "--file",
                &lock.to_string_lossy(),
                "upstroke.residue",
                "interrupted",
            ],
        );
        assert!(lock.is_file(), "the lock is planted");

        // Control: the residue really does block an index write.
        let worktree = fixture.manager.slot_path(&fixture.task);
        let blocked = git_fixtures::git(&worktree, &["add", "-A"]);
        assert_ne!(
            blocked.code,
            Some(0),
            "an index.lock that blocks nothing would make this test vacuous"
        );
        assert!(
            blocked.stderr.contains("index.lock"),
            "and it is the lock that blocked it: {}",
            blocked.stderr
        );

        // The claim.
        let mut hooks = Hooks::new();
        fixture
            .manager
            .remove_worktree(hooks.effects(), &fixture.task)
            .expect("forced removal reclaims through administrative residue");

        assert!(!worktree.exists(), "the checkout is gone");
        assert!(
            !admin.exists(),
            "the row is gone, and the residue left with it: {}",
            admin.display()
        );
        assert!(!lock.exists(), "including the lock itself");
        assert!(
            !git_fixtures::git_ok(&fixture.base, &["worktree", "list", "--porcelain"])
                .contains("kalpha-g0"),
            "and Git no longer lists it"
        );
    }

    /// `cleanup`: "snapshots pruned on completion and **reclaimed as
    /// residue**", and `T-SCRUB`'s "snapshot intents reclaimed".
    ///
    /// Both halves, because they are two mechanisms: a live coordinator removes
    /// the snapshot it created, and a process that died holding one leaves an
    /// intent that the next process's reclaim finds. The ephemeral commit each
    /// snapshot created returns to R27 when its snapshot goes, which is the
    /// object half of the same sentence.
    #[test]
    fn snapshot_residue_reclaimed() {
        let fixture = Fixture::new("snapshot-residue");
        let mut hooks = Hooks::new();
        let input = SnapshotInput::Tree {
            tree: fixture.tree_sha.0.clone(),
            parent: fixture.base_sha.0.clone(),
        };

        // One snapshot for the gate set, one for the reviewer: `snapshots` says
        // they are never reused across roles, so the reclaim has two rows.
        let gates = fixture
            .manager
            .add_snapshot(hooks.effects(), &SnapshotName::gates(0, 1), &input)
            .expect("the gate snapshot");
        let review = fixture
            .manager
            .add_snapshot(hooks.effects(), &SnapshotName::review(0, 1, 0), &input)
            .expect("the reviewer's snapshot");
        let ephemeral = gates.ephemeral.clone().expect("a tree input commits");
        assert_eq!(
            ephemeral,
            review.ephemeral.clone().expect("and so does the other"),
            "the same tree on the same parent is the same commit"
        );
        assert!(
            !fixture.is_unreachable(&ephemeral),
            "while a snapshot has it checked out it is R24, not R27"
        );

        // Half one: the live removal.
        fixture
            .manager
            .remove_snapshot(hooks.effects(), &gates)
            .expect("prune the gate snapshot");
        assert!(!gates.path.exists());
        assert!(!fixture.manager.intent_path(&gates.slot).exists());

        // Half two: the reviewer's snapshot is left as a dead process would
        // leave it, and the next process's reclaim finds it by its intent.
        let reclaimed = fixture
            .manager
            .reclaim_intents(hooks.effects())
            .expect("reclaim");
        assert!(
            reclaimed.contains(&review.slot),
            "the reviewer's snapshot is reclaimed as residue: {reclaimed:?}"
        );
        assert!(!review.path.exists(), "its worktree is gone");
        assert!(
            !fixture.manager.intent_path(&review.slot).exists(),
            "and so is its intent"
        );

        // The object half: with no snapshot and no ref holding it, the
        // ephemeral commit is Git's again.
        assert!(
            fixture.is_unreachable(&ephemeral),
            "the ephemeral snapshot commit returns to R27"
        );
        assert!(
            fixture.object_present(&ephemeral),
            "returned to R27, not deleted"
        );
        assert!(
            hooks.observed(
                EffectSiteId::Snapshot(SnapshotSite::Remove),
                HookPhase::Before
            ) && hooks.observed(
                EffectSiteId::Snapshot(SnapshotSite::RemoveIntent),
                HookPhase::Before
            ),
            "both snapshot removal sites ran through their funnels"
        );
    }

    // =======================================================================
    // The names, the refusals, and the window between the append and the prune
    // =======================================================================

    /// The two ref names are durable identity: they are written into
    /// `candidate_prepared` and a resume rebuilds them from the same inputs. So
    /// they are pinned against literals rather than against the function that
    /// builds them.
    #[test]
    fn the_candidate_refs_are_the_names_the_packet_gives() {
        let names = CandidateNames::of("01RUN", TaskKey(7), GenerationId(3));
        assert_eq!(
            names.prepared_ref.as_str(),
            "refs/upstroke/runs/01RUN/candidate-prepared/7/3"
        );
        assert_eq!(
            names.candidate_ref.as_str(),
            "refs/upstroke/runs/01RUN/candidates/7/3"
        );
        assert_eq!(
            names.prepared_ref,
            candidate_pin_ref("01RUN", TaskKey(7), GenerationId(3))
        );
        assert_eq!(
            names.candidate_ref,
            candidates_ref("01RUN", TaskKey(7), GenerationId(3))
        );

        // The namespace's trailing separator, which is what keeps run `01RUN`
        // from owning run `01RUNNER`'s refs.
        assert_eq!(run_namespace("01RUN"), "refs/upstroke/runs/01RUN/");
        assert!(
            names
                .prepared_ref
                .as_str()
                .starts_with(&run_namespace("01RUN"))
        );
        assert!(
            !candidate_pin_ref("01RUNNER", TaskKey(7), GenerationId(3))
                .as_str()
                .starts_with(&run_namespace("01RUN")),
            "a sibling run's namespace is not a prefix of this one's"
        );
    }

    /// `T-CAND-REF`'s "object missing": the candidates ref is created only for
    /// a commit that is actually in the repository.
    ///
    /// The refusal is what stops a resume from creating an authoritative ref
    /// out of a record whose object a `git gc` already collected — which is a
    /// reachable state precisely because the pin is what kept it alive.
    #[test]
    fn promotion_refuses_a_commit_that_is_not_in_the_repository() {
        let fixture = Fixture::new("object-missing");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        let absent = CommitSha("0123456789abcdef0123456789abcdef01234567".to_owned());
        assert!(
            !fixture.object_present(absent.as_str()),
            "and it really is absent"
        );
        let forged = PromotingCandidate {
            candidate: CandidateRef {
                key: ALPHA,
                generation: GENERATION,
                commit_sha: absent.clone(),
                candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
            },
            prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
            base: fixture.base_sha.clone(),
            tree: fixture.tree_sha.clone(),
        };
        let refused = complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            forged,
        )
        .expect_err("an absent object refuses");
        assert!(
            refused.to_string().contains(absent.as_str())
                && refused
                    .to_string()
                    .contains("not an object in this repository"),
            "{refused}"
        );
        assert!(
            fixture.run_refs().is_empty(),
            "and it refuses before creating anything: {:?}",
            fixture.run_refs()
        );
        assert_eq!(journal.count("task_candidate_created"), 0);
    }

    /// **Present is not the same as *is the candidate*.**
    ///
    /// DESIGN.md §15 has `candidate_prepared` record the complete
    /// attempt/base/commit/tree identity "so resume adopts only the judged
    /// object". A promotion that asks only whether *something* is at that SHA
    /// adopts whatever is there, and the two things that can be there are the
    /// two this asserts: an object that is not a commit at all, and a commit
    /// that is not on the generation's base. Both exist; neither is the judged
    /// candidate; both must refuse before the ref is created.
    ///
    /// The tree is deliberately **not** asserted, and it is not an oversight:
    /// the fold keeps the candidate, the base and the paths, so a resume has no
    /// recorded tree to compare against. `PR7-CANDIDATE-TREE-UNVERIFIED` in
    /// `reviews/FINDINGS.md` §2 is that residue, recorded rather than papered
    /// over.
    /// **A commit on the recorded base, carrying a tree nobody judged, is refused.**
    ///
    /// `DESIGN.md` §15: `candidate_prepared` records "exactly one complete
    /// attempt/base/commit/tree identity … so resume adopts only that exact shape".
    /// Recovery checked existence and parent. Both pass here — the impostor *is* a
    /// commit and its parent *is* the base — and its tree is a tree no gate ran
    /// against and no reviewer read. Adopting it would create the authoritative
    /// candidate ref at that object and append `task_candidate_created`, which is
    /// the whole of what the merge queue then trusts.
    ///
    /// The sibling above refuses objects that are not commits, or commits that are
    /// not on the base. Neither reaches this: the difference here is **content**,
    /// and content is what the ladder judged.
    ///
    /// Raised by the frontier re-review of `c2c0294` as finding B and carried
    /// before that as `PR7-CANDIDATE-TREE-UNVERIFIED`. The repair is
    /// `PreparedCandidate::tree_sha`, per-instance Class B approval 2026-08-26.
    #[test]
    fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {
        let fixture = Fixture::new("tree-never-judged");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        let (impostor, impostor_tree) = fixture.divergent_tree_commit(&mut hooks);
        assert_ne!(
            impostor_tree, fixture.tree_sha,
            "the fixture built the judged tree again, so there is nothing to refuse"
        );

        // Both of the checks that existed pass, stated rather than assumed —
        // otherwise this test could be green because an *earlier* refusal fired.
        assert!(
            fixture
                .manager
                .object_exists(impostor.as_str())
                .expect("cat-file"),
            "the impostor is not present, so the existence check refuses it first"
        );
        assert_eq!(
            fixture
                .manager
                .commit_parent(impostor.as_str())
                .expect("rev-parse")
                .as_deref(),
            Some(fixture.base_sha.as_str()),
            "the impostor is not on the recorded base, so the parent check refuses \
             it first and the tree is never reached"
        );

        let forged = PromotingCandidate {
            candidate: CandidateRef {
                key: ALPHA,
                generation: GENERATION,
                commit_sha: impostor.clone(),
                candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
            },
            prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
            base: fixture.base_sha.clone(),
            // The durable record's tree — what the fold now retains.
            tree: fixture.tree_sha.clone(),
        };
        let Err(refused) = complete_promotion(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            forged,
        ) else {
            panic!(
                "recovery adopted a commit whose tree is {} where the record says {}: the \
                 authoritative candidate ref would name an object nothing judged",
                impostor_tree.0, fixture.tree_sha.0
            )
        };
        assert!(
            refused.to_string().contains(impostor.as_str()),
            "the refusal must name the object it refused: {refused}"
        );

        // And nothing was appended or created on the way to refusing.
        assert_eq!(
            journal.count("task_candidate_created"),
            0,
            "a refused adoption still took a queue position"
        );
        assert!(
            fixture
                .manager
                .direct_ref_target(candidates_ref(RUN_ID, ALPHA, GENERATION).as_str())
                .expect("read the ref")
                .is_none(),
            "a refused adoption still created the authoritative candidate ref"
        );
    }

    #[test]
    fn promotion_refuses_an_object_that_is_not_the_judged_candidate() {
        for (label, present) in [
            ("a tree, not a commit", true),
            ("a commit that is not on the base", false),
        ] {
            let fixture = Fixture::new("object-not-the-candidate");
            let mut hooks = Hooks::new();
            let mut journal = fixture.journal(&hooks);

            // Two real objects of the fixture's own repository. The tree is not
            // a commit; the base is a commit whose parent is not the base.
            let impostor = if present {
                CommitSha(fixture.tree_sha.0.clone())
            } else {
                fixture.base_sha.clone()
            };
            // The **production** presence predicate, not the fixture's: the
            // residue classifier asks `cat-file -e <sha>^{}`, which resolves any
            // object, so a tree answers "present" there. Asserting it here is
            // what makes this a test of identity rather than of existence — the
            // check being repaired would have passed both of these.
            assert!(
                fixture
                    .manager
                    .object_exists(impostor.as_str())
                    .expect("cat-file"),
                "{label}: the impostor is present to the residue classifier, so existence alone \
                 would pass"
            );

            let forged = PromotingCandidate {
                candidate: CandidateRef {
                    key: ALPHA,
                    generation: GENERATION,
                    commit_sha: impostor.clone(),
                    candidate_ref: candidates_ref(RUN_ID, ALPHA, GENERATION),
                },
                prepared_ref: candidate_pin_ref(RUN_ID, ALPHA, GENERATION),
                base: fixture.base_sha.clone(),
                tree: fixture.tree_sha.clone(),
            };
            let Err(refused) = complete_promotion(
                &fixture.manager,
                &mut hooks,
                &mut journal,
                &fixture.task,
                forged,
            ) else {
                panic!("{label}: an impostor object must refuse");
            };
            assert!(
                refused.to_string().contains(impostor.as_str()),
                "{label}: {refused}"
            );
            assert!(
                fixture.run_refs().is_empty(),
                "{label}: and it refuses before creating anything: {:?}",
                fixture.run_refs()
            );
            assert_eq!(journal.count("task_candidate_created"), 0, "{label}");
        }
    }

    /// `git commit-tree` printing something that is not a full object id is
    /// refused before any ref primitive sees it.
    ///
    /// Reached directly because the real command cannot produce it: an id that
    /// `update-ref` would read as a *name to resolve* rather than as an error
    /// is the shape `workspace_manager::Refusal::MalformedObjectId` exists for,
    /// and this is the same guard one step earlier, where the value enters.
    #[test]
    fn a_commit_id_that_is_not_a_full_object_id_refuses() {
        for value in ["", "abc", "HEAD", "0123456789abcdef0123456789abcdef0123456"] {
            let refusal = refuse_malformed_commit(ALPHA, GENERATION, &CommitSha(value.to_owned()))
                .expect_err("not a full hexadecimal object id");
            assert!(
                refusal.to_string().contains("full hexadecimal object id"),
                "{value:?}: {refusal}"
            );
        }
        refuse_malformed_commit(
            ALPHA,
            GENERATION,
            &CommitSha("0123456789abcdef0123456789abcdef01234567".to_owned()),
        )
        .expect("forty hexadecimal characters is an object id");
    }

    /// The window O31 opens: `task_candidate_created` is durable and the pin it
    /// should have pruned is not yet pruned.
    ///
    /// `cleanup` says pins are "pruned right after promotion (**or as
    /// orphans**)", so this state has to be recoverable — and it is the one
    /// state a classifier that only looked at the *open* generation would miss,
    /// because the append that reached it also closed the generation.
    ///
    /// Reached by an error return rather than a kill: the claim is about what
    /// the next process finds, and both leave the same durable prefix, so the
    /// cheaper one is the honest choice. `Ref.DeleteCandidatePin`'s `Before` is
    /// a reachability phase in the frozen registry rather than a declared fault
    /// coordinate; the module-local double injects there to reach the prefix,
    /// which is scaffolding, not a claim about the registry.
    #[test]
    fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {
        let fixture = Fixture::new("pin-left-behind");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);

        let unpinned =
            write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
                .expect("commit-tree");
        let commit = unpinned.commit_sha().clone();
        let pinned = pin_candidate(&fixture.manager, &mut hooks, unpinned).expect("pin");
        hooks.arm_phase(
            EffectSiteId::Ref(RefSite::DeleteCandidatePin),
            HookPhase::Before,
            Injection::Error,
        );
        promote(
            &fixture.manager,
            &mut hooks,
            &mut journal,
            &fixture.task,
            pinned,
        )
        .expect_err("the prune was made to fail");

        // The prefix: the queue position is durable, the generation is closed,
        // and the pin is still there.
        assert_eq!(journal.count("task_candidate_created"), 1);
        assert_eq!(journal.generation_class(), Some(GenerationClass::Closed));
        let mut names: Vec<String> = fixture
            .run_refs()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0,
                candidates_ref(RUN_ID, ALPHA, GENERATION).0,
            ],
            "the pin outlived the promotion"
        );
        // …and the namespace does not refuse it, which is what lets the next
        // process act at all.
        fixture
            .manager
            .refuse_unexpected_refs(
                &run_namespace(RUN_ID),
                &expected_refs(RUN_ID, journal.fold()),
            )
            .expect("a pin the promotion has not pruned yet is not an unexpected ref");

        // The closure procedure finishes it, appending nothing.
        let recovery =
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");
        assert!(
            !recovery.settles_interrupted && recovery.orphan_pin.is_none(),
            "a durable candidate is not an orphan and its attempt is settled: {recovery:?}"
        );
        let promoting = recovery.promotion.expect("the promotion is unfinished");
        assert_eq!(
            promoting.prepared_ref(),
            &candidate_pin_ref(RUN_ID, ALPHA, GENERATION)
        );

        let mut clean = Hooks::new();
        let queued = complete_promotion(
            &fixture.manager,
            &mut clean,
            &mut journal,
            &fixture.task,
            promoting,
        )
        .expect("the closure procedure finishes it");
        assert_eq!(queued.candidate().commit_sha, commit);
        assert_eq!(
            journal.count("task_candidate_created"),
            1,
            "and appends nothing: the generation is no longer Promoting"
        );
        assert_eq!(
            fixture.run_refs(),
            vec![(candidates_ref(RUN_ID, ALPHA, GENERATION).0, commit.0)],
            "the pin is pruned and the authoritative ref is untouched"
        );
        assert!(
            recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
                .expect("classify again")
                .is_empty()
        );
    }

    /// `T-CAND-OBJ`'s other `refusal_condition`: "**pin symbolic** or an
    /// unexpected ref under the run namespace".
    ///
    /// Both the writer and the reader refuse it. That matters separately: a
    /// symbolic pin that only the writer refused would still be followed by the
    /// resume that reads it, and `INV-17`'s "every engine ref is direct" would
    /// hold on the way in and not on the way out.
    #[test]
    fn a_symbolic_pin_refuses_on_both_the_write_and_the_read() {
        let fixture = Fixture::new("symbolic-pin");
        let mut hooks = Hooks::new();
        let journal = fixture.journal(&hooks);
        let pin = candidate_pin_ref(RUN_ID, ALPHA, GENERATION);
        git_fixtures::git_ok(
            &fixture.base,
            &["symbolic-ref", pin.as_str(), "refs/heads/main"],
        );

        let unpinned =
            write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())
                .expect("commit-tree");
        let refused = pin_candidate(&fixture.manager, &mut hooks, unpinned)
            .expect_err("a symbolic pin refuses");
        assert!(
            refused.to_string().contains("symbolic ref")
                && refused.to_string().contains("refs/heads/main"),
            "the refusal names what it found: {refused}"
        );

        let read = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)
            .expect_err("and the resume refuses to read it too");
        assert!(read.to_string().contains("symbolic ref"), "{read}");
    }

    /// A ref under the run namespace that no durable record accounts for is
    /// refused — which is the half of `T-CAND-OBJ`'s refusal condition that
    /// needs [`expected_refs`] to have derived something.
    ///
    /// Two shapes, because the derivation has two rules and one of them is
    /// tighter. A candidates ref for a generation that **exists and has
    /// prepared nothing** is the shape a derivation that expected both names
    /// for every generation would wave through; a candidates ref for a
    /// generation that does not exist at all is the shape any derivation
    /// catches. Only the first measures the rule.
    #[test]
    fn an_unexpected_ref_under_the_run_namespace_refuses() {
        let fixture = Fixture::new("unexpected-ref");
        let mut hooks = Hooks::new();
        let mut journal = fixture.journal(&hooks);
        let namespace = run_namespace(RUN_ID);
        let candidates = candidates_ref(RUN_ID, ALPHA, GENERATION);

        // (1) The generation exists and has prepared no candidate, so it is
        // entitled to a pin and to nothing else.
        assert_eq!(
            expected_refs(RUN_ID, journal.fold()),
            vec![candidate_pin_ref(RUN_ID, ALPHA, GENERATION).0],
        );
        git_fixtures::git_ok(
            &fixture.base,
            &["update-ref", candidates.as_str(), fixture.base_sha.as_str()],
        );
        let refused = fixture
            .manager
            .refuse_unexpected_refs(&namespace, &expected_refs(RUN_ID, journal.fold()))
            .expect_err("no candidate is prepared, so no candidates ref is accounted for");
        assert!(
            refused.to_string().contains("unexpected ref")
                && refused.to_string().contains(candidates.as_str()),
            "{refused}"
        );
        git_fixtures::git_ok(&fixture.base, &["update-ref", "-d", candidates.as_str()]);

        // (2) After the promotion the same name is accounted for, and the
        // namespace holds exactly what the fold says it may.
        run_to_queued(&fixture, &mut hooks, &mut journal);
        let expected = expected_refs(RUN_ID, journal.fold());
        assert!(expected.contains(&candidates.0));
        fixture
            .manager
            .refuse_unexpected_refs(&namespace, &expected)
            .expect("what promotion left is what the fold accounts for");

        // (3) …and a ref for a generation that never existed still refuses.
        let stowaway = candidates_ref(RUN_ID, ALPHA, GenerationId(9));
        git_fixtures::git_ok(
            &fixture.base,
            &["update-ref", stowaway.as_str(), fixture.base_sha.as_str()],
        );
        let refused = fixture
            .manager
            .refuse_unexpected_refs(&namespace, &expected)
            .expect_err("generation 9 does not exist");
        assert!(refused.to_string().contains(stowaway.as_str()), "{refused}");
    }

    fn attempt_record() -> AttemptRecord {
        AttemptRecord {
            attempt: 1,
            tier: "mid".to_owned(),
            model: "alpha-model".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::from_millis(1_234),
            cost_usd: Some(0.5),
            // The primary pass §11.2 requires, present and passed. Empty
            // `reviews` satisfies `is_successful` vacuously — the premise then
            // exercises none of the clause it is the positive witness for.
            reviews: vec![ReviewRecord {
                pass: "review".to_owned(),
                agent: "claude-code".to_owned(),
                model: "claude-opus-5".to_owned(),
                adapter: Some("claude-code".to_owned()),
                preflight_cli_version: None,
                effort: None,
                pool: None,
                cost_usd: None,
                outcome: ReviewPassOutcome::Passed,
            }],
            session_id: None,
            usage: None,
            failure: None,
        }
    }
}
