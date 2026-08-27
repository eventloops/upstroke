# PR7 frontier review, round 3 — `bf927f3`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED** |
| Reviewed SHA | `bf927f32e71d09a8361d4cad3ddf464860c28883` |
| Previously | `c2c0294` ([record](2026-08-26-pr7-frontier-review-c2c0294.md)), `75da796` ([record](2026-08-26-pr7-frontier-review-75da796.md)) |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort = max`, read-only sandbox |
| Inputs | worktree at the reviewed SHA; `pr.diff`; `pr.md`; both prior records; `delta-since-last-review.diff` (`502970d..bf927f3`) |
| CI at that SHA | 10/10 success, uncancelled |
| Diff | **66 files** — assembled by the driver from this pull request's own base, with no caller override |

## The harness finding is dismissed, and the reviewer verified the diff itself

> *"`pr.diff` is the exact `615597c...bf927f3` diff: both SHA-256 values are
> `7565472c…`, covering 66 files. Prior finding D was a review-harness artifact and is
> dismissed."*

The round-2 driver repair worked unprompted: `gh pr diff` hit the API's 20,000-line cap,
and the driver assembled the diff from the pull request's own base rather than from
`master`. The reviewer hashed both sides rather than taking that on trust.

## Three P1 findings, each re-derived here

### 1. `attempt_finished{Succeeded}` **and** `candidate_prepared` are both appended

`decisions/2026-08-12-merge-queue-execution-topology.md:155`, an immutable record:

> `candidate_prepared`: the **sole** successful settlement for an attempt that produces a
> candidate. … **`attempt_finished` is not also emitted for that attempt.**

The driver appends both. `promote_candidate` calls `settle_succeeded`, emits
`TopologyEventBody::AttemptFinished`, and then runs `append_candidate_prepared` →
`create_candidates_ref` → `append_candidate_created`.

**This slice already knew the behaviour and documented it as correct.** `Spend::replay`
carries bespoke deduplication whose own comment reads *"for a **successful** attempt both
are appended"* — the workaround is evidence of the duplicate, not a licence for it.

The reviewer's sequence: `attempt_finished{Succeeded}` is durable, the process dies before
`candidate_prepared`, another Git writer replaces the pin with a same-parent commit, and
recovery — treating a present pin as the successful settlement's candidate — **writes a new
`candidate_prepared` derived from that commit**. Finding B's tree check then passes,
because recovery itself recorded the substituted tree.

**This is a conflict between the implementation and a live authority, not a bug in a
function.** Either the decision record is right and the driver must stop appending
`attempt_finished` for candidate-producing attempts — which changes the fold, the spend
reader, and every witness that counts settlements — or the record is superseded and says
so. That is an owner ruling.

### 2. Recovery never compares the prepared pin's **target** to the durable record

`recovery_for` reads the pin and uses it as `pin.is_some()`; the target is never compared
with `prepared.candidate.commit_sha`. A pin substituted from `C` to `X` after
`candidate_prepared` therefore leaves resume promoting the durable `C`, appending
`task_candidate_created`, and then **deleting the substituted pin expected-old** — erasing
the evidence of substitution rather than refusing while preserving it, which is what §15's
extended exact-identity rule requires.

Finding B's repair does not reach this: it binds the commit to its tree, not the pin to the
record.

### 3. Fresh-run P4 accounts one probe where the adapter runs ten processes

`create.rs` registers `PreflightIdentities::agent(agent, 0)` around the whole adapter call.
A Codex probe executes ten Runner requests. If the version probe at ordinal 0 succeeds and
a help probe at ordinal 1 fails, the ledger records the **successful** process as cancelled
and holds no record of the one that failed.

**Resume already solves exactly this and says why.** `preflight.rs` wraps the Runner:

> *"The run's Runner with R3 and R4 wrapped around every request. One place, so that 'each
> a registered invocation' is true of a process an adapter built as much as of one this
> module built."*

Fresh creation does not use that boundary. The reviewer notes this also falsifies a
**shipped claim** in `reviews/2026-08-25-pr7-g2-evidence.md:198` that every probe process is
individually registered, and a stale statement there that Codex runs four processes.

## A, B and C

**A — behaviour closed; the witness claim is false.** The reviewer confirms the repair
works: the legacy caller chooses `LadderEvent`, schema 4 chooses `AttemptRecord`, the full
feedback no longer reaches the legacy record or `report.json`, and the strict-door rationale
for the `"detail":null` residual is sound.

But the body and the decision record say the witness *compares against captured `610106b`
bytes*, and it does not. The captured fixture appears only as elided prose in the doc
comment; the test strips the null, parses the remainder, checks three key names and a
`reason.starts_with(...)` predicate. Changing the reason's suffix still passes. The
`report.json` half asserts `failure["detail"].is_null()`, which cannot distinguish an
explicit null from an absent key.

**The behaviour is repaired and the claim about how it is held is overstated** — which is
this slice's own recurring class, in the repair for that class.

**B — closed.** `+20/−0` on the frozen file, 18 doc lines and two of code, nothing
serde-visible moved, and the witness proves the pre-existing checks pass on its
divergent-tree commit before asserting the refusal. It does not close findings 1–2.

**C — not closed, three ways, all verified here.**

| claim | is |
|---|---|
| "**Seven shapes** spend nothing … a `FailureShape` count — **not** a `FailureKind` count" | `FailureShape` **is** `(kind, origin)`. Thirteen pairs spend nothing, spanning **seven kinds**. The doc names the shape count and states the kind count. Fourth wrong version of this sentence. |
| the test computes that count | it builds the 13 pairs and then collapses them to a `BTreeSet` of **kind names**, asserting 7. It computes the kind count — a different quantity from the one the doc claims. |
| "`Interrupted` is `FailureKind`'s first variant and `Declined` its last; a new variant between them fails this list to compile" | **inverted**: `NoChain` is first, `Interrupted` is last. And a hand-written 14-element array does not fail to compile when an enum gains a variant, so the exhaustiveness guard does not exist. |
| "`events::Dangling::event` builds the other `AttemptRecord`" | **no such type.** It is `InterruptedAttempt::event`. The name was invented in two places — `classify.rs` and `events/mod.rs` — inside the correction of a false claim about that very constructor. |

## The macOS red

The reviewer read the disclosure and declined to block on it:

> *"I would not block specifically on the disclosed macOS red: the same-SHA rerun passed,
> the signature matches an existing rated row, and `327cce3...bf927f3` changes only
> `reviews/FINDINGS.md`. That does not cure the open residue defect, but it is not
> introduced here."*

## Other rule checks it performed

- No pre-existing dated decision record is modified; this pull request adds only
  `decisions/2026-08-26-durable-retry-feedback.md`.
- The schema-4 facade is `pub(crate)` and `create_run` has only test callers, so the narrow
  `production_effect = none` claim is true.
- No newly added panicking `unwrap`/`expect`, non-binary `anyhow`, or non-`std::path`
  filesystem handling in the production files it read.
- `git diff --check` clean. It did not run the suite — the workspace is read-only.

## Coverage declaration, verbatim

Files read in full:

- `pr.md`
- `prior-review-1-75da796.md`
- `prior-review-2-c2c0294.md`
- `DESIGN.md`
- `decisions/2026-08-26-durable-retry-feedback.md`
- `src/engine/attempt.rs`
- `src/engine/classify.rs`
- `src/engine/topology.rs`
- `src/engine/topology/attempt.rs`
- `src/engine/topology/create.rs`
- `src/engine/topology/emit.rs`
- `src/engine/topology/preflight.rs`
- `src/engine/topology/recover.rs`
- `src/engine/topology/run.rs`
- `src/ladder.rs`

Files read in part:

- `pr.diff`
- `delta-since-last-review.diff`
- `decisions/2026-08-12-merge-queue-execution-topology.md`
- `reviews/2026-08-25-pr7-g2-evidence.md`
- `reviews/2026-08-25-pr7-standing-questions.md`
- `reviews/FINDINGS.md`
- `src/agent/codex.rs`
- `src/agent/mod.rs`
- `src/effects.rs`
- `src/engine/coordinator.rs`
- `src/engine/mod.rs`
- `src/engine/report.rs`
- `src/engine/tests.rs`
- `src/engine/topology/candidate.rs`
- `src/engine/topology/create/tests.rs`
- `src/engine/topology/dispatch.rs`
- `src/engine/topology/emit/tests.rs`
- `src/engine/topology/identity.rs`
- `src/engine/topology/prelock.rs`
- `src/engine/topology/recover/tests.rs`
- `src/engine/topology/scaffold.rs`
- `src/engine/topology/seams.rs`
- `src/engine/topology/select.rs`
- `src/engine/topology/settle.rs`
- `src/engine/topology/startup.rs`
- `src/events/log.rs`
- `src/events/mod.rs`
- `src/lib.rs`
- `src/runner/container/exec.rs`
- `src/runner/mod.rs`
- `src/topology/events.rs`
- `src/topology/fold.rs`
- `src/util.rs`
- `src/workspace_manager.rs`

Changed files not read:

- `.gitignore` — would check whether unrelated generated artifacts or review evidence were newly ignored.
- `decisions/README.md` — would check the new decision’s index entry and chronology.
- `effects/wrappers.toml` — would inspect changes to effect-funnel classification or enforcement.
- `reviews/2026-08-24-unfreeze-challenge-request.md` — would verify the frozen-layer authorization’s exact scope.
- `reviews/2026-08-25-pr7-standards-worklist.md` — would route nonblocking standards findings and check they were not presented as contract repairs.
- `reviews/2026-08-26-pr7-frontier-review-75da796.md` — would compare the repository copy with the supplied prior-review record.
- `reviews/2026-08-26-pr7-frontier-review-c2c0294.md` — would compare the repository copy with the supplied second-review record.
- `reviews/2026-08-26-pr7-s5-closing-sweep.md` — would inspect the sweep’s corpus and whether every claimed measurement was actually in scope.
- `src/capacity.rs` — would check shared-authority moves for changed routing or spend behavior.
- `src/effects/tests.rs` — would evaluate whether the effect censuses die under missing or duplicated funnel sites.
- `src/engine/assembly.rs` — would check moved worker-plan construction for legacy/schema-4 divergence.
- `src/engine/topology/attempt/tests.rs` — would inspect end-to-end worker, gate, review, and process-ledger witnesses.
- `src/engine/topology/dispatch/tests.rs` — would check reservation-before-dispatch and retry-generation witnesses.
- `src/engine/topology/preflight/tests.rs` — would specifically look for a fresh-run multi-process adapter case exposing the P4 accounting defect.
- `src/engine/topology/run/tests.rs` — would check every driver arm and successful candidate-sequence witness.
- `src/engine/topology/startup/tests.rs` — would inspect census, lock, and retained-husk recovery witnesses.
- `src/events/log/tests.rs` — would check torn-tail and ambiguous-append detection, including identical adjacent lines.
- `src/gates.rs` — would check moved gate construction and feedback-tail behavior.
- `src/review.rs` — would check review ordering, re-asks, and required-changes propagation.
- `src/rundir.rs` — would inspect creation/removal boundaries and Windows path handling.
- `src/runner/container.rs` — would check shared container-runner behavior and Windows retry bounds.
- `src/runner/container/census/tests.rs` — would inspect concurrent reclaimer witnesses and their bounded retry assumptions.
- `src/runner/container/fake.rs` — would check pre-clean ownership filtering and fixture fidelity.
- `src/runner/container/resolve/tests.rs` — would inspect resolution and environment-composition witnesses.
- `src/runner/container/tests.rs` — would inspect container identity, cleanup, and refusal tests.
- `src/runner/container/view.rs` — would check role-scoped Git-view construction and path containment.
- `src/status.rs` — would check whether duplicate successful record-bearing events are deduplicated consistently.
- `src/topology/registry.rs` — would verify the mechanical `detail: None` initializers and no registry behavior change.
- `src/workspace.rs` — would inspect moved capture/diff helpers for legacy behavior changes.
