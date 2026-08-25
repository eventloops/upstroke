# PR7 — the six standing questions

**Head:** `f6ed9f1` · Answered against the tree and
`cumulative_review_gates.standing_questions`, under G2's remit: *"answer Q1–Q6 for the
candidate pipeline at max_parallel = 1 (Q2 durable rows in range only)."*

Every claim below names the test that holds it. Where the answer is **scoped**, the
scope is the contract's, quoted.

---

## Q1 — reconstruction

**Can every fold-derived runtime state be reconstructed from durable state alone?**

**Yes, and it is the only path.** `TopologyFold` has no constructor that takes state:
`replay(inputs, events)` folds the log, and `plan_transition`/`apply_delta` is the single
transition. `live_and_replay_reach_the_same_state_over_a_long_trace` and
`every_guarded_event_is_refused_the_same_way_live_and_on_a_hostile_replay` are what make
that a property rather than an intention; `engine::tests::live_state_equals_replayed_state_across_every_ladder_path`
holds the same line for schema 3.

The states the question enumerates are each in the fold: `RetainedIdle { session,
incarnation }` carries both retained fields; `GenerationClass` distinguishes `Promoting`;
`CandidateQueue` carries position and `verification_deferred`; `LeaseTable`, open
questions, binding overrides, `halted_at` and the epoch-scoped budget stop are readers on
`TopologyFold`.

**One reconstruction gap was found and closed in this slice.** The **deferral count** was
written into the log by `SettlementTransition::Deferred { defers }` and never accumulated
onto the task — `TaskFold` had no such field and `TaskState::Deferred` is a unit variant.
A driver keeping its own tally agreed with the log on every reading except the one after a
resume, where it read zero while the log held three, and the run would have deferred past
its allowance forever. `TaskFold::defers` closes it, and
`a_deferral_count_is_derived_by_replay_and_not_by_a_process_local_tally` is the witness.
Disclosed as `PR7-FOLD-DEFERS-ACCUMULATOR` (`reviews/FINDINGS.md` §3).

**Process-lifetime holdings provably empty at process start.** `Reservations`,
`SlotAssertion` and `InvocationLedger` are constructed empty by `TopologyRun::resumed` and
have no deserialization path — there is no way to carry one across a process. The
surviving Unix reaper's shared hold is the exception the question anticipates: it is
**observed and refused-until-released**, never reset
(`the_reapers_cleanup_hold_is_shared_between_overlapping_invocations`, R28).

**Durable recovery records reclaimed or repaired before any reuse.** The recovery order is
eleven steps (a0)–(i) carried as a type, `RecoveryStep::ALL`, so an omitted step fails to
compile rather than passing silently — the guard this project adopted after
`PR7-RECOVERY-STEP-G-MISSING`, where a packet-named step had no implementation and 117
tests were green.

**The stable-prefix barrier precedes every fold-derived effect.**
`resume_establishes_stable_prefix_barrier_before_any_fold_derived_effect`,
`the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold` — the strongest
form, since it makes the barrier the *only* constructor — and
`checked_replay_consumes_exactly_the_reread_bytes`. Its four predicates are refused
independently (`the_stable_prefix_barrier_refuses_each_of_its_four_predicates_independently`).

**Private root and Runner rebuild** are PR4/PR6's and unchanged by this slice.

---

## Q2 — resource accounting

*(Durable rows in range only, per the remit.)*

**Yes for the rows PR7 owns.** The slice's `owned_resources` names R9, R24, R5, R7, R6,
R23, R11, R27, plus R1/R3/R4/R13/R17/R21/R28 as assertions or observations. Each has one
class in `decisions.resource_accounting` and one inventory row; `effect_sites.json`
carries the `row` field per site and the bijection is checked by
`the_bijection_over_the_whole_claimed_inventory_fails_for_this_slice` — a test whose name
says it is measuring the claim rather than assuming it.

**Engine-created Git objects** are accounted by the reference that holds them and released
to R27 once nothing does: staged blobs and trees stay index-referenced until the scrub, an
ephemeral snapshot commit is left to Git once its snapshot is removed, and a candidate
commit before its pin — or with its id unread — is left to Git. That last is why
`IdUnread` is a distinct point on two sites rather than a hook phase.

**Entitlement grants** span processes and are derived from recorded active generations
and unresolved transactions; slot, provisional, invocation and lock-hold grants are
per-process and start empty (Q1). At `max_parallel = 1` R3 is an assertion, not a broker
— stated by the contract, not by this build.

**The outcome equations, closure and terminal finalization are G5's**, and PR7 refuses
run-end closure beyond the checkpoint refusal. `derived_outcome` is total over the
explored state (`the_derived_outcome_is_total_over_every_explored_state`,
`the_derived_outcome_is_total_over_the_crossed_fold_state`) but nothing in this slice
acts on it.

---

## Q3 — stale completions

**No.** Every mutating event is refused unless its identity is the current open one. The
fold's checks are by name — a settlement for a closed or mismatched generation or attempt
is a `FoldError`, and this is in `pr_sequence[8].expected_failures_refusals`: *"fold
refusals for closed-generation, wrong-attempt, or stale-incarnation settlements"*.

The **incarnation** rule is the sharpest case: `RetainedIdle { incarnation }` refuses a
retry from any process but the one that retained the session, which is `T-RETAINED`'s
*"the retaining incarnation proceeds to T-RETRY; a fresh process closes it in recovery"*.
A mutation that re-announced a retry's attempt was refused by the fold with the generation
class in the message — measured, in the commit that landed the retry branch.

**Can a returned append error let a stale in-memory fold drive a later mutation?** No, and
this slice strengthened the guarantee. Obligation (1) of the append-error protocol poisons
the fold explicitly, and `EmitState.fold.poison()` is the only caller. Obligation (3) —
cancelling in-flight invocations — **moved to the caller** in this slice, and the report is
now unreachable without discharging it: `AppendError` carries a private witness field and
`UncancelledAppend::cancelling(&mut InvocationLedger)` is its only constructor in the
crate. `From<EmitError> for UpstrokeError` was deliberately deleted, because it read the
report and would have let any `?` stringify an outstanding obligation and drop it.
`the_production_emitter_reaches_the_append_error_protocol` asserts all five obligations
with the caller in the loop, and dies to the transplant mutation.

---

## Q4 — aliasing

**No, for the resources in range.** Names are derived, not chosen: `task_slot(key,
generation)` is the only worktree slot constructor, `CandidateNames::of(run_id, key,
generation)` the only pin and candidates-ref constructor, and `AttemptIdentities::new(key,
generation, attempt)` the only invocation-identity source, with `InvocationLedger`
asserting each is registered once and settled once (R4).

**Path leases** are the case this slice got wrong once and fixed, and it is the strongest
evidence here. `fold::predicted_region` strips glob metacharacters to a literal prefix; a
second derivation written engine-side took hints literally, so for `src/alpha/*.rs` the
fold admitted on `src/alpha` while the log recorded `src/alpha/*.rs` — a prefix
overlapping nothing, which would have let two tasks hold overlapping regions.
`PR7-REGION-SECOND-DERIVATION`. The driver now reads the fold's answer and never forms
one; the assertion that keeps it honest is in
`the_driver_takes_over_from_the_recovery_order_and_steps`, whose fixture hint is
deliberately a glob.

**Can a resume write into a private half that is not provably its own?** No — the
bidirectional ownership proof gates every private deletion, and a half that cannot be
proven is retained and reported rather than removed
(`resume_reclaims_a_provable_husk_beside_the_run_and_retains_a_possibly_committed_one`,
`the_resume_census_reports_the_husk_it_could_not_reclaim`).

**Can a fast publication name an object other than the judged candidate commit?**
Out of range — fast publication is PR8's, and this slice refuses integration at the
checkpoint.

---

## Q5 — effect/append boundaries

**Yes for the sites in range, with one exception recorded rather than absorbed.**

`effect_sites.json` declares **70 sites** across eleven groups; every one either has a
funnel that names it or is recorded absent
(`every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`),
and the generated inventory describes every site and invents none
(`the_generated_inventory_describes_every_site_and_invents_none`). Both hook phases and
the parent-executed sub-effect points are offered in every declared mode
(`every_append_point_is_offered_in_every_mode_the_frozen_inventory_declares`,
`every_open_point_is_offered_in_every_mode_the_frozen_inventory_declares`,
`every_kill_point_the_inventory_declares_has_a_case_and_no_case_is_invented`).

**Residue classes**: synthetic construction per element, plus a real-command kill-sampling
record — see §3 of `reviews/2026-08-25-pr7-g2-evidence.md`, including the caveat that the
sampler has no seed.

**The exception.** Legacy's third cheap rung,
`Workspace::review_input_problem_for_tree`, reads a `Workspace`; the driver holds a
`WorkspaceManager` and a `Slot`. It is reached through the `ReviewInputPolicy` seam rather
than re-derived, and the legacy engine was made the seam's **first** caller so there is one
policy and one classification with two callers, not two of each.

**Disallowed list, allow-placement scan, wrapper classification, allowlists** — all
present and passing; see §7 of the evidence document. **The dependency review against
`Cargo.lock` in range is G2's own**, per `claim_scope`: *"G2 and G5 reviewers confirm the
lists against the code and Cargo.lock in range."* Not answerable from this side.

---

## Q6 — sequential guarantees

**Scoped, and the scope is the contract's.** Q6 asks whether *parallel* execution
preserves the sequential guarantees. **PR7 has no parallel execution**:
`decisions.sequential_substrate.engine` is *"TopologyRun drives schema 4 at max_parallel =
1 synchronously; every path exists here before Tokio"*, and
`sequential_substrate.tokio_boundary` assigns concurrency to PR11 with G6 as its gate. So
the answerable half is: **do the sequential guarantees hold at one?**

**Yes, for the ones in range.**

- **One fold, one writer.** The worktree lock is the first effect of every write command
  after its read-only pre-lock checks and is held across the census and the whole run
  (R17); a second coordinator is refused.
- **Exact-commit verification on immutable snapshots.** Gates run on one shared exact
  snapshot and each reviewer on a fresh one, all from the captured tree, never the live
  worktree.
- **Untouched user checkout.** Q5's §5 evidence.
- **Atomic settlements — with a correction.** An earlier draft of this answer said "one
  settlement per attempt". That is **not** what the code does, and S5's `contract` lens was
  right to raise it. A *successful* attempt appends **two** events carrying its
  `AttemptRecord`: `attempt_finished{Closed{Succeeded}}`, which is the only thing that
  moves the generation to `Promoting`, and `candidate_prepared`, which
  `check_candidate_prepared` refuses in any other class. The two-append sequence is forced
  by the fold, not chosen by the driver.

  INV-07's "candidate_prepared is the sole successful attempt settlement" is about which
  event records the **candidate**, not about which event settles the attempt — the
  distinction `settle_succeeded`'s own documentation draws. What *is* atomic is the
  decision: the transition, the parking, the deferral count, the lease disposition and the
  allowance are decided once, by `settle_failed` or `settle_succeeded`.
  `GenerationLease::expected` is the whole of the lease rule and
  `check_lease_disposition` refuses any other answer.

  **A consequence a G2 reviewer must not be left to discover.** Because both events carry
  the record, anything that walks the log counting records will price a successful attempt
  twice. `Spend::replay` did exactly that until S5 round 1; it now counts one contribution
  per `(key, generation, attempt)`, and
  `a_runs_spend_is_the_same_live_as_on_replay` asserts live-versus-replay parity rather
  than a corrected number.
- **`budget_exceeded` before any budget-driven end.** `loop`: a breach *"appends
  `budget_exceeded` before any effect"*, and the append and the refusal are deliberately
  two iterations so the record of the breach is durable before the closure it causes.
- **One Runner identity per run**, recorded before the first probe and rebuilt by every
  resume — PR4/PR6's, unchanged here.

**Out of range, and refused rather than approximated:** FIFO integration, no overtaking,
exact-base fast publication (PR8); closure, finalization and the derived-outcome ordering
(`halt > budget > parked > complete`) beyond the checkpoint refusal (PR10); and **answer
ingestion after halt or budget stop in the same epoch** — the whole of answer ingestion is
PR9's, which this slice's `LoopBranch::IngestAnswers` carries as
`Disposition::NotThisSlice` with the contract passages that assign it.
