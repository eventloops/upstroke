# PR8 plan — integration transactions

Branch `feat/pr8-integration-transactions`, cut from `master` at `44a33caa`. Contract:
`decisions.pr_sequence[9]` of `tactus-parallel-design-neutral-v16.json`, as amended by
`2026-08-25-g2-pass-errata.md` (the errata win on conflict; none of E1–E6 touches PR8's own
sentences, so the JSON text of the slice contract is the text implemented). Brief:
`START-PR8.md` as amended on 2026-09-06 (no pull request, no review, no findings lane; the body
goes to `pr8-body.md`). Implementation model: Claude Fable 5.1 at max effort.

This file is the standing plan and the record of every reading taken where the packet was
ambiguous. Each reading is a decision, not a question. It is updated as commits land.

## 1. Reading report

### What already exists (nothing below is re-implemented)

- The whole event vocabulary of the slice is PR3's and is on the wire already:
  `MergeVerificationStarted{sequence, candidate, basis: StaleClean{prepared_ref} | AlreadyPresent,
  expected_head, proposed_sha}`, `MergeVerificationUnavailable{sequence, cause, outcome}`,
  `MergeVerificationInterrupted`, `MergePrepared{disposition: Fast | StaleClean |
  AlreadyPresent, expected_head, proposed_sha, candidate_sha, candidate_ref, prepared_ref,
  verification_source, verification, satisfies}`, `MergeRejected{disposition: Conflict{paths} |
  CodeRejected{verification}, repair: FrozenSpawn, lease_effect}`, `TaskMerged{sequence,
  merged_sha, satisfies, lease_release}`, `DeferWaitElapsed`.
- The checked fold already refuses the slice's relations: one transaction at a time, dense
  sequences, first-eligible start, the exact-base case refused as a verification, the three
  `merge_prepared` relation families (fast / stale_clean / already_present), `satisfies` closure
  equality, Deferred at `max_defers`, non-consecutive defers, Parked without a complete question,
  HumanRequired without Parked, `task_merged` against a non-authorized transaction, lease
  release shape, and the repair registration shape (dense key, lineage root/parent/index,
  deps Merged, ladder well-formed, admission consistent with the entry).
- The funnels exist: staging slot `merge/s<seq>` (`Worktree.WriteStagingIntent/AddStaging/
  RemoveStaging/RemoveStagingIntent`), `Object.ProposalCherryPick`, `Ref.PinPrepared`,
  `Ref.DeletePreparedPin`, `Ref.CompareAndSwapIntegration`, exact snapshots of a commit
  (`SnapshotInput::Commit`, "no new object"), `assert_publishable`, `direct_ref_target`,
  `refs_under`, `refuse_unexpected_refs`, the residue classifier for the cherry-pick residue
  class, and the stable-prefix barrier of recovery step (a1) with its typed witness chain.
- The loop selects `Step::Integrate` for the first eligible candidate and the checkpoint refuses
  it; recovery refuses an unresolved transaction. Both refusals are PR7's and are replaced.

### What the slice obliges (in my words)

1. **Selection and reservation.** Ceiling check before selection (already in `select`); a
   provisional `{pipeline, merge}` reservation (`ReservationKind::Integration`, 2 entitlements)
   taken before any staging effect and converted at the first append — `merge_prepared(fast)`,
   `merge_verification_started`, or `merge_rejected(conflict)` — or cancelled on any pre-append
   failure, leaving the candidate queued and only reclaimable residue.
2. **The exact-base decision** is made under `assert_publishable` from the integration ref head
   `H` read through `direct_ref_target`, before any staging effect. `H == candidate.base_sha`
   (the base `candidate_prepared` recorded) is fast; anything else is stale.
3. **Fast**: `merge_prepared(fast){expected_head: H, proposed_sha: candidate.commit,
   prepared_ref: None, verification_source: CandidatePrepared{key, generation}, verification:
   None, satisfies}` → CAS `H -> candidate.commit` → `task_merged`. No staging worktree, intent,
   cherry-pick, object or pin at any point of the sequence; the hook harness records none of
   `Worktree.AddStaging`, `Object.ProposalCherryPick`, `Ref.PinPrepared` inside the fast
   sequence; `git fsck` object count unchanged; no `merge/s<seq>` intent ever written.
4. **Stale**: staging intent → `add_worktree(staging, H)` → `proposal_cherry_pick(candidate)`:
   clean → `prepared/<seq>` pin zero-old at the proposal → `merge_verification_started
   {StaleClean{prepared_ref}, expected_head: H, proposed_sha: proposal}`; empty → `merge_
   verification_started{AlreadyPresent, expected_head: H, proposed_sha: H}`; conflict →
   `merge_rejected{Conflict{paths}}` with the complete frozen repair. Verification runs every
   recorded gate on one fresh exact snapshot of the proposal (or head) commit and every review
   pass on its own fresh snapshot, through the Runner, with `InvocationId::sequence(seq, role,
   ordinal)` identities; the staging worktree runs nothing.
5. **Terminals** of `merge_verification_started`, each implemented: `merge_prepared(stale_clean |
   already_present)` carrying the passing verification record → CAS → `task_merged`;
   `merge_rejected{CodeRejected{verification}}` with the frozen repair; `merge_verification_
   unavailable{HumanRequired{verdict}, Parked{question}}`; `merge_verification_unavailable
   {Infrastructure{kind}, Deferred{defers} | Parked{question}}`; `merge_verification_interrupted`
   appended by resume for a dangling start.
6. **Repair registration** inside `merge_rejected`: complete `FrozenSpawn` (key = registry.len(),
   display id `merge-fix-<index>-<root>`, origin MergeRepair, kind Fix, evidence in the spec,
   original acceptance plus preserve-merged-behaviour, path hints widened by the candidate's
   actual paths and the conflict paths, `min_tier = mid` intersected with the root's frozen
   floor/pin/ceiling, root's deps, root's reviews, probed agents, lineage {root, parent, index}),
   admission Runnable | HumanRequired{limit} (over the frozen `max_merge_repairs`) |
   HumanBinding{options} (empty intersection), and the lease effect (CreatesLineage from an
   ordinary candidate, WidensLineage for a member).
7. **Cleanup after the terminal**: snapshots removed as each role finishes; the staging worktree
   and its intent removed with force after the terminal; the prepared pin deleted expected-old
   at Deferred/Parked/interrupted terminals and, for a prepared publication, after `task_merged`.
8. **Recovery** rows T-FAST, T-PROPOSAL, T-VERIFY, T-PREPARED, T-REJECT(registration): every
   CAS or promotion only after the barrier of step (a1); a `Prepared` transaction resolves by
   ref == expected → CAS then `task_merged`, ref == proposed → `task_merged`, anything else
   refuses (third SHA), symbolic or checked out refuses through `assert_publishable`; a
   `VerificationStarted` transaction settles `merge_verification_interrupted`, deletes its pin
   expected-old, and the candidate is re-verified under a new sequence; staging and snapshot
   residue is reclaimed with force from intents; the exact orphan `prepared/<next_seq>` is
   reclaimed expected-old and a prepared pin outside the sequences the log pinned refuses.
9. **Checkpoint refusals** (part of the slice): dispatch of a Repair-origin task and
   repair-admission answers are refused before any append, with the citation pattern of
   `run.rs`/`recover.rs`. Run-end closure stays refused (PR10).
10. **Verification-park questions** are PR8's: the permitted transitions name "parked verification
    Answered -> AwaitingMerge; Declined -> Failed (queue position consumed; lease released or
    lineage failed; halting per decline_halts_run)", and the proof tests name park/answer/decline.
    So the loop ingests answers to *verification-park* questions and refuses answers to
    *repair-admission* questions before any append.

### Readings taken where the text is ambiguous or silent

- **R1. Where the prepared pin of a published proposal is pruned.** R12 says "pruned by
  finalization when prepared" while `cleanup` says "prepared pins pruned after terminal" and the
  site `Ref.DeletePreparedPin` is `After(task_merged)`. Reading: delete the pin expected-old
  right after `task_merged` (the object stays reachable from the integration ref); a pin that
  survives a kill between `task_merged` and its deletion is pruned by the next resume at its
  recorded proposal sha. PR10's finalization stays free to prune whatever is left.
- **R2. `already_present`'s "validation-only no-op CAS".** Reading: issue the real expected-old
  update `update-ref --no-deref <ref> H H` through `Ref.CompareAndSwapIntegration`, so the
  expected head is validated atomically by Git and the site is observed; nothing moves.
- **R3. Empty cherry-pick detection.** Git reports an already-present change as a failed
  cherry-pick (exit 1, `CHERRY_PICK_HEAD` left, index equal to HEAD, no unmerged paths). Reading:
  the funnel `proposal_cherry_pick` is unchanged; a read-only inspection `proposal_state`
  classifies the staging worktree after a failed pick into Conflict{unmerged paths} or Empty,
  and any other failed state is the Git error it was. The residue a failed pick leaves in the
  staging git dir leaves with the forced removal after the terminal.
- **R4. Which failures are infrastructure at integration.** Reviewer `Unavailable` with
  RateLimited → `Infrastructure{RateLimited}`; reviewer `Timeout` → `ReviewerTimeout`; any other
  reviewer unavailability → `ReviewUnavailable`; a Runner error spawning a gate or reviewer →
  `RunnerSpawnFailure`. A gate that exits non-zero or times out is code-attributed
  (`GatesFailed`); a reviewer verdict that rejects is `Rejected`; a reviewer that asks for a
  human is `HumanRequired{verdict: reasons}`; a review input that cannot be judged (too large or
  opaque) is `HumanRequired` too — a Fix task cannot be asked to edit code without code evidence
  and waiting cannot make the same diff fit.
- **R5. Defer arithmetic.** `Deferred{defers}` with `defers = queued.defers + 1` while that is
  `< max_defers`; at `>= max_defers` the outage parks (so `max_defers = 0` parks the first outage).
  This is exactly what `check_defer_allowance` accepts.
- **R6. `merge_rejected` lease paths.** Conflict → the conflict paths; code rejection → the
  candidate's actual paths (the region the rejected code touched). Both are unioned with the
  candidate's held region by the fold.
- **R7. Over-limit and empty-intersection at once.** The fold refuses a `HumanRequired`
  admission on a `HumanBinding` entry, so one admission must win: the empty intersection wins
  (`HumanBinding`), because without a binding nothing can run whatever the limit says.
- **R8. Counting automatic repairs.** INV-11 says the fold counts automatic rejections per root;
  the fold today only checks the recorded limit value. Reading: the registered lineage members
  of a root are its automatic rejections (each `merge_rejected` registers exactly one); a
  Runnable or HumanBinding admission is refused once `members >= max_merge_repairs`, and a
  HumanRequired admission is refused below it. Enforced in the fold (Class B, below) and derived
  identically by the driver.
- **R9. Repair path hints.** Original hints ∪ candidate actual paths ∪ conflict paths, as
  strings; a `RepoWide` region contributes nothing (an absent-or-unreadable region widens by
  nothing, and the lease already holds repo-wide).
- **R10. Repair ladder.** Tiers of the root's frozen ladder at or above `max(Mid, root floor)`
  and at or below its ceiling, with the root's rungs for exactly those tiers; `floor` recorded
  as that maximum, `ceiling` as the highest surviving tier (`None` when none survives);
  `attempts_per`, effort and reviews copied from the root. Empty survivors → `rungs: []`,
  `Admission::HumanBinding{options: probed_agents}`.
- **R11. Repair deps.** The root's authoritative deps (`repairs.lineage`), all Merged by
  construction; display deps copied alongside.
- **R12. Verification-park question.** `FrozenQuestion{id: ids.question_id(), key: candidate.key,
  kind: Clarify for a reviewer's needs-human, Unblock for an unjudgeable input or an outage at
  max_defers, context from the coordinator's question builder with the verification failure,
  options from `question_options(kind)`}`. Questions carry no attribution and no `DesignDefect`
  is emitted.
- **R13. When answers are ingested.** `AnswerSource::resolve` may block (terminal or file source
  with a wait budget), so answers are read where the legacy engine reads them: at the hard
  block, when nothing else is runnable. A verification-park answer appends
  `question_answered{Answered{option_index, binding_override: None}}` (the chosen option's
  index, or 0 for free text) or `{Declined{decline_halts_run}}`; a repair-admission answer is
  refused before any append (PR9). `decline_halts_run` follows the run's `on_task_failure`
  policy, carried into `RunSeams` beside `halts_run`.
- **R14. Residue reclaim at resume.** No PR7 step reclaims snapshot or staging intents (task
  worktrees are verified or recreated by (g)). Reading: a residue-reclaim step after the census
  and before the runner is rebuilt removes, with force, every `snapshots/*` and `merge/*` intent
  and its worktree; orphan pins stay in step (f) with the candidate orphan pins because they are
  ref work, which the existing order performs only after the Runner is rebuilt.
- **R15. Expected refs at resume.** Prepared pins expected under the run namespace are exactly:
  `prepared/<seq>` for every sequence whose `merge_verification_started` has a StaleClean basis
  (derived from the proven prefix's events), plus `prepared/<next_seq>` (the possible
  provisional orphan). A pin of a resolved sequence is pruned expected-old at its recorded
  proposal sha and refused at any other sha; a pin at any other sequence is refused by
  `refuse_unexpected_refs` ("orphan pin outside next sequence").
- **R16. Expected head after `merge_prepared`.** CAS recovery needs the authorized
  `expected_head`; the fold's `TransactionClass::Prepared` did not retain it. Reading: retain it
  in the fold (Class B) rather than re-derive it from the event list a second time.
- **R17. Snapshot names for integration reviewers.** `SnapshotName::integration(seq)` is the
  gate snapshot; a new `SnapshotName::integration_review(seq, pass)` names one fresh snapshot
  per reviewer (`workspace_manager/naming.rs`, not a frozen path).
- **R18. Implementer binding for `passes_for`.** The candidate's `AttemptRecord` records tier
  and model; the agent is the root entry's rung at that tier (the run froze one agent per tier).
- **R19. The two-crash proof's "unsynced merge_prepared".** Constructed with the event funnel's
  `Written` kill in `Complete` shape (a whole line reached the file, no fsync), the durability
  ledger recording what was synced; "power loss" is a truncation of the log to the length the
  ledger proves durable. The barrier's own fsync at the next open is what makes the line
  survive, and the test measures that through the ledger rather than assuming it.
- **R20. Kill sampling of the cherry-pick child.** The residue class is already proven at the
  funnel by PR5's sampler; PR8 adds the engine-level recovery: synthetic residue (unreferenced
  object, `CHERRY_PICK_HEAD`, `index.lock`) in a staging git dir converges under the resume's
  forced reclaim, and a sampled run of `git cherry-pick` children killed at uncontrolled points
  in a staging worktree classifies every sample and converges the same way.

## 2. Staged implementation plan (one commit per shape)

Each commit is gated locally (fmt, clippy, the touched test families) before the next; the full
ten-command baseline runs before every push.

| # | Commit | Production | Tests it lands | Ordering assertion it rests on |
|---|---|---|---|---|
| 0 | `docs(pr8): plan` | this file | — | — |
| 1 | `feat(topology): fold readers and the retained expected head` | `predicates.rs`: `next_sequence`, `satisfies_closure`, `lineage_members`; `TransactionClass::Prepared.expected_head`; INV-11 count in `check_merge_rejected` | fold tests: readers agree with the checks; over-limit admission refused both ways, live and on replay | none (state only) |
| 2 | `feat(engine): fast integration` | `integrate.rs`: selection → reservation → `assert_publishable` + head read → fast `merge_prepared` → CAS → `task_merged`; loop branch `Integration` performed; `checkpoint` admits `Integrate`, refuses Repair-origin dispatch; `expected_refs` gains pins | `fast_path_publishes_exact_candidate_without_staging_or_proposal_object` (real repo; harness fast sequence; fsck count; no intent), `merge_prepared_fast_with_moved_head_or_wrong_proposed_or_pin_refused_live_and_on_replay`, `third_sha_refused`, symbolic/checked-out refusals, `fast_dual_holding_released_once` (ST-13), repair-origin dispatch refused before append, replay twice equal | reservation before the head read; `merge_prepared` before CAS; CAS before `task_merged`; no staging site observed in the sequence |
| 3 | `feat(engine): stale_clean and already_present verification` | staging intent → add → cherry-pick → `proposal_state`; pin; `merge_verification_started`; verification on commit snapshots (gates + reviewers with sequence identities, `AttemptPlans::verification`); `merge_prepared(stale_clean/already_present)` → CAS → `task_merged` → pin delete; forced staging removal | `stale_candidate_takes_staging_path_and_publishes_pinned_proposal`, `already_present_recovery_no_empty_commit` (no commit manufactured; CAS new == old), `merge_prepared_stale_clean_with_unpinned_proposed_refused`, verification isolation (snapshots of the proposal commit, no new object, nothing runs in staging), ST-04 sequence aliasing (second start refused; occupied staging path refused) | staging intent before add; proposal objects before the pin; pin before `merge_verification_started`; snapshot intents before snapshot add; `merge_prepared` before CAS; removal after the terminal |
| 4 | `feat(engine): merge_rejected with atomic repair registration` | `repair.rs`: the frozen spawn, admission, lease effect; conflict and code-rejected terminals | conflict → lineage created, repair Pending, parent AwaitingRepair, lease transferred; code rejection (gate failure, review rejection); member rejection widens; over-limit → HumanRequired; empty intersection → HumanBinding; `kill_after_merge_rejected_neither_loses_nor_duplicates_repair`; replay twice equal | `merge_rejected` before any repair effect (no dispatch, no worktree); reservation released at the terminal |
| 5 | `feat(engine): unavailable terminals` | Deferred/Parked for Infrastructure and HumanRequired; pin deletion and staging removal at the terminal; defer wake through the existing backoff branch | `human_required_verdict_parks_task`, `infrastructure_failure_defers_then_parks_at_max_defers`, `defer_wait_elapsed_reenables_deferred_candidate`, non-eligible start refused for deferred and parked candidates, Deferred at max / non-consecutive / Parked without question / HumanRequired without Parked refused live and on replay | terminal before pin deletion and staging removal; both entitlements released at the terminal |
| 6 | `feat(engine): verification-park answers and decline` | hard-block ingestion of verification-park answers; refusal of repair-admission answers; `decline_halts_run` seam | park/answer/decline tests; `declined_parked_verification_fails_task_consumes_queue_position_releases_lease_and_halts_per_policy`; repair-admission answer refused before append | answer append before any re-verification; decline before any release |
| 7 | `feat(engine): integration recovery` | recovery step (f): `merge_verification_interrupted`, CAS recovery after the barrier, orphan and resolved pins, residue reclaim of staging and snapshot intents; `refuse_unimplemented_terminals` narrowed to what PR8 still refuses | `kill_between_prepared_and_cas`, `kill_between_cas_and_merged`, `kill_during_merge_verification_settles_interrupted_and_reverifies`, `kill_after_proposal_commit_before_pin_reclaims_staging_and_leaves_object_to_git`, `orphan_prepared_pin_reclaimed_only_at_next_sequence`, `staging_residue_reclaimed`, synthetic cherry-pick residue converges, sampled cherry-pick child kills classified and recovered, both `Object.ProposalCherryPick` hook phases, Event points (kill and error) at the new appends, `prepared_publication_completed_at_run_end` for the resume path | barrier before any CAS; `merge_verification_interrupted` before pin deletion; no CAS on a replay-visible-only `merge_prepared` |
| 8 | `test(engine): the two-crash proof` | — | `unsynced_merge_prepared_two_crash_barrier_before_cas_then_power_loss_keeps_log_and_ref_agreeing`, `barrier_sync_failure_before_cas_issues_no_cas_and_converges_after_loss` | barrier (sync, stable reread, checked replay) before the CAS |
| 9 | `test(engine): terminal-shape coverage table and ST-13` | — | the eight-row table driven end to end; ST-13 sequential subset incl. the fast no-staging assertion; replay-twice-equal over every shape | — |
| 10 | `docs(pr8): body, design notes, internals` | `pr8-body.md`; DESIGN §26 additions only if a sentence the code enforces is missing; `docs/internals` notes for new modules if the gate requires them | gates | — |

Commits may be split further; none is merged with another. The order of commits 2–5 is by
terminal shape as the brief asks; 7 and 8 close the recovery rows the earlier commits opened.

## 3. `src/topology/**` changes: Class A / B / C

Rule applied: a read-only accessor that exposes a derivation the fold already makes is
**Class A** (self-serve, disclosed here and in `pr8-body.md`); anything that changes what the
fold accepts, refuses, retains or applies is **Class B** (owner approval owed before landing);
any change to the wire vocabulary would be **Class C** (none planned). Where a change could be
read either way it is listed as B.

| Change | File | Class |
|---|---|---|
| `TopologyFold::next_sequence()`, `satisfies_closure(key)`, `lineage_members(root)` readers | `src/topology/fold/predicates.rs` | A |
| `TransactionClass::Prepared` retains `expected_head` (set in `apply_merge_prepared`; read by CAS recovery) | `src/topology/fold.rs`, `src/topology/fold/apply.rs` | B |
| `check_merge_rejected` counts a lineage's registered repairs against `max_merge_repairs` and refuses the wrong admission (INV-11) | `src/topology/fold/check_integration.rs` | B |
| Fold tests for the above | `src/topology/fold/tests.rs` | A (tests only) |

No other frozen path is expected to change. If the implementation forces one, it is added to
this table and to `pr8-body.md` before the commit that needs it lands.

## 4. Blocking items

None recorded yet.
