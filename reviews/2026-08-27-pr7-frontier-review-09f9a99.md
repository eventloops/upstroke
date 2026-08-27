# PR7 frontier review, round 4 — `09f9a99`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED** |
| Reviewed SHA | `09f9a99ad55fc27ab7d28a066e8f6bad8b75fb4a` |
| Previously | `bf927f3`, `c2c0294`, `75da796` (records in this directory) |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort = max`, read-only sandbox |
| Inputs | worktree at the reviewed SHA; `pr.diff` (68 files); `pr.md`; **all three** prior records; `delta-since-last-review.diff` (`cfe2471..09f9a99`) |
| Legs at that SHA | gates clean (1712 + 8, 0 failed); Windows guest 1669 + 10, 0 failed; CI 10/10, uncancelled |
| Diff | verified byte-identical to `git diff 615597c...09f9a99`, sha256 `fedf5ba0…`; the delta likewise |

**Every finding below was re-derived here before being relayed. All five are accurate, and
all five are defects in the round-3 repairs.**

## Prior-item disposition, as the reviewer states it

| # | verdict |
|---|---|
| 1 settlement move | **not closed** — the deletion is sound and the ordering witnesses kept their counts, but the allowance mutation was dropped, source comments still prescribe the forbidden pair, and ~15 tests were "re-derived" by neutering a shared helper |
| 2 pin binding | **closed** — both recorded-candidate paths bind; leaving the orphan path unbound is correct |
| 3 probe accounting | **not closed** — `Request` can hold different locks than `RunnerProbes`, and the witness bypasses `create_run` |
| A legacy witness | **closed** — the whole raw object is compared after removing exactly one `,"detail":null`, and both halves distinguish an explicit null from an absent key |
| C shape count | **closed for the current authority** — 13 over 7, the source reader finds 14 variants, rejects an empty parse, and maps through an exhaustive match |

## The findings, each verified

### 1 (P1). Moving the settlement dropped the rung-allowance mutation

`apply_settlement` increments `TaskFold::attempts_on_rung` when `spends_allowance` says the
attempt spent one — and a successful attempt has `failure: None`, for which
`spends_allowance` answers **true**. Only `AttemptFinished` reaches that function.
`apply_candidate_prepared` records the candidate and sets `Promoting` and **does not
increment**:

```
$ sed -n '2054,2130p' src/topology/fold.rs | grep -c 'attempts_on_rung'
0
```

So a task at `attempts_per = 2` whose second attempt succeeds ends at
`attempts_on_rung = 1`; a first-attempt success leaves it at **zero**. Replay reproduces
the undercount, and a later allowance reader can grant an extra attempt.

**This contradicts the Class B approval in its own words** — *"settlement counting moves to
the sole event"* — which is the text I wrote authorising the change, and then did not
implement. The replacement witness asserts `Promoting` and candidate presence and cannot
see the gap; the allowance census still finds the syntactic increment in `apply_settlement`
and passes without proving the new settlement reaches it.

### 2 (P1). The fold's new door accepts a record that says the attempt failed

`check_candidate_prepared` validates attempt number, base, parent and lease and mentions
`failure` **zero times**. So a `candidate_prepared` whose embedded `AttemptRecord` carries
`failure: Some(GateFailed)` is accepted, promotes the generation, and is carried to
`task_candidate_created` — a task durably queued as a successful candidate whose own
authoritative evidence says a gate failed.

The whole point of the Class B change was that this event *is* the successful settlement.
The fold is the authority against malformed or faulty future writers, and it does not
enforce the condition that motivated the change. It also invalidates `Brief::replay`'s
assumption that a `candidate_prepared` record never carries a failure.

### 3 (P1). Fresh-creation accounting can be checked against the wrong locks

`RunnerProbes` holds ledger/slot references; `Request` independently accepts a
`&dyn Probes` **and** its own ledger/slot references. Nothing requires them to be the same
pair. Construct the probes over locks A and the `Request` over empty locks B, and P4 runs
through A while the closing balance assertion reads B and reports a vacuous pass.

**The witness cannot catch it**: it calls `RunnerProbes::agent` directly and inspects that
wrapper's own locks. It never constructs `Request` and never calls `create_run`, so it
proves the wrapper and not the end-to-end accounting the repair claimed. This is the same
shape as the two gaps round 2's battery found — a witness that starts downstream of the
step it is about.

### 4 (P1, docs-contract). The ruling was not propagated, and the "never patched" claim is false

Five production comments still describe the dual settlement the fold now refuses —
`candidate.rs`'s module header, `attempt.rs`, `run.rs` (which repeats the *rejected*
reinterpretation of INV-07 and links the deleted `settle_succeeded`), `settle.rs`, and
`recover.rs`, which describes the deleted `complete_promotions` as still appending.

**A fourth fabricated identifier**, which the prompt asked to be treated as a finding:
`CandidateRecovery::SettleInterrupted` — `CandidateRecovery` is a struct with a
`settles_interrupted: bool` field and no such associated item. And `events::Dangling::event`,
reported corrected "in both files", survives in
`decisions/2026-08-26-durable-retry-feedback.md:76` — the **immutable** artifact, and the
one a reader reaches first.

**And the witness claim.** `Journal::settle_succeeded` was turned into an explicit no-op and
kept at ~15 call sites. That is patching a shared helper so its callers pass, not
re-deriving each sequence, and the round-3 commit and ledger say the witnesses were
re-derived and never patched. The three named ordering arrays *were* correctly shortened;
the general claim is false.

### 5 (P2). The body's exact-head, scope, rollback and stamp claims

- Validation still reads *"Local, at `327cce3`— the head this body describes"*: seven
  commits behind, predating all five repairs. Scope and Review evidence were updated to
  `09f9a99` and Validation was not.
- *"no event kind, serialization, or transition changed"* is now false twice over: the
  legacy schema-3 failure object gains `"detail":null` — which this branch's own byte
  witness proves — and the accepted schema-4 transition changed from
  `attempt_finished{Succeeded} → candidate_prepared` to direct `candidate_prepared`. A log
  this head writes is **refused by the immediately preceding fold**.
- The G2 correction is stamped `8f0e605`, which touches no `create.rs`; the creation repair
  is `35aaf8e`, one commit later. Verified: `git show --stat 8f0e605` has no `create.rs`.
- *"A substitution refuses without touching anything"* is too strong. It holds for
  `recovery_for`. On the late window — substitution after the candidate ref and
  `task_candidate_created` are written — `reclaim_after_creation` refuses and preserves the
  pin, but only after those effects. The security property is repaired; the absolute claim
  is not.

## What the reviewer checked and did not dispute

`pr.diff` byte-identical to the base-correct diff and the delta likewise; the frozen-file
totals match the body, including the settlement approval's `+152/−81` split; no landed
decision record edited; no new production `unwrap`/`expect`, non-binary `anyhow`, or
non-`std::path` handling; `git diff --check` clean. It did not re-run the gates — the
workspace is read-only — and explicitly does not dispute the supplied exact-head results,
while noting they exercise neither the cross-wired ledger nor the missing allowance
mutation.

## Coverage declaration, verbatim

Coverage universe: all 68 paths in the exact PR diff, the supplied artifacts, and the two governing authority/decision files consulted.

Files read **IN FULL**:

- `DESIGN.md`
- `pr.md`
- `prior-review-1-75da796.md`
- `prior-review-2-c2c0294.md`
- `prior-review-3-bf927f3.md`
- `delta-since-last-review.diff`
- `.gitignore`
- `decisions/README.md`
- `decisions/2026-08-12-merge-queue-execution-topology.md`
- `decisions/2026-08-26-durable-retry-feedback.md`
- `src/engine/mod.rs`
- `src/engine/topology.rs`
- `src/ladder.rs`

Files read **IN PART**:

- `pr.diff` — exact-byte comparison, path inventory, global rule scans, and targeted hunks.
- `reviews/2026-08-25-pr7-g2-evidence.md`
- `reviews/FINDINGS.md`
- `src/capacity.rs`
- `src/engine/classify.rs`
- `src/engine/coordinator.rs`
- `src/engine/tests.rs`
- `src/engine/topology/attempt.rs`
- `src/engine/topology/candidate.rs`
- `src/engine/topology/create.rs`
- `src/engine/topology/create/tests.rs`
- `src/engine/topology/preflight.rs`
- `src/engine/topology/recover.rs`
- `src/engine/topology/recover/tests.rs`
- `src/engine/topology/run.rs`
- `src/engine/topology/select.rs`
- `src/engine/topology/settle.rs`
- `src/events/mod.rs`
- `src/runner/mod.rs`
- `src/status.rs`
- `src/topology/census.rs`
- `src/topology/events.rs`
- `src/topology/fold.rs`
- `src/topology/registry.rs`

Changed files **NOT READ**:

- `effects/wrappers.toml` — would verify every newly shared reader/wrapper has the narrow effect classification claimed.
- `reviews/2026-08-24-unfreeze-challenge-request.md` — would compare each frozen-file mutation against its original authorization.
- `reviews/2026-08-25-pr7-standards-worklist.md` — would check that only nonblocking standards matters were routed there.
- `reviews/2026-08-25-pr7-standing-questions.md` — would check whether current contract defects were omitted from the standing set.
- `reviews/2026-08-26-pr7-frontier-review-75da796.md` — would byte-compare the repository copy with the supplied first-review record.
- `reviews/2026-08-26-pr7-frontier-review-c2c0294.md` — would byte-compare the repository copy with the supplied second-review record.
- `reviews/2026-08-26-pr7-s5-closing-sweep.md` — would inspect its corpus and whether its measurements support its exact scope.
- `reviews/2026-08-27-pr7-frontier-review-bf927f3.md` — would byte-compare the repository copy with the supplied third-review record.
- `src/effects.rs` — would inspect production-region parsing and whether the censuses can omit new topology call sites.
- `src/effects/tests.rs` — would check that corpus controls actually traverse the production census implementations.
- `src/engine/assembly.rs` — would check shared builder moves for legacy/schema-4 value-flow divergence.
- `src/engine/attempt.rs` — would inspect reviewer feedback construction and failure attribution.
- `src/engine/topology/attempt/tests.rs` — would inspect full worker/gate/review and invocation-accounting witnesses.
- `src/engine/topology/dispatch.rs` — would check reservation-before-dispatch and generation ownership.
- `src/engine/topology/dispatch/tests.rs` — would inspect dispatch, retry, and overlap mutation witnesses.
- `src/engine/topology/emit.rs` — would inspect append-error ownership and poison/cancellation typing.
- `src/engine/topology/emit/tests.rs` — would inspect exact append-prefix and error-path assertions.
- `src/engine/topology/identity.rs` — would check ordinal, role, and slot identity uniqueness.
- `src/engine/topology/preflight/tests.rs` — would inspect per-process registration and slot-release tests independently of creation.
- `src/engine/topology/prelock.rs` — would inspect runner resolution and the creator’s pre-lock witnesses.
- `src/engine/topology/run/tests.rs` — would inspect all driver branches and candidate-sequence end-to-end traces.
- `src/engine/topology/scaffold.rs` — would check whether the shared fixture bypasses production checks.
- `src/engine/topology/seams.rs` — would inspect whether test seams can violate invariants production types enforce.
- `src/engine/topology/startup.rs` — would inspect the eventual assembly point for matching `Request` and `RunnerProbes` locks.
- `src/engine/topology/startup/tests.rs` — would inspect census, lock, and retained-husk ordering witnesses.
- `src/events/log.rs` — would inspect stable-prefix and append-error behavior after the error-type move.
- `src/events/log/tests.rs` — would inspect legacy/new funnel differential coverage.
- `src/gates.rs` — would check exact-tree gate execution and portable command/path handling.
- `src/review.rs` — would inspect review-input policy and `run_review` value flow.
- `src/rundir.rs` — would inspect Windows-safe run paths and husk classification.
- `src/runner/container.rs` — would inspect container runner registration and platform boundaries.
- `src/runner/container/census/tests.rs` — would inspect the carried Windows bounded-read failure and reclaim convergence.
- `src/runner/container/exec.rs` — would inspect invocation/slot propagation and container path handling.
- `src/runner/container/fake.rs` — would inspect pre-clean namespace scoping.
- `src/runner/container/resolve/tests.rs` — would inspect resolved policy and image identity witnesses.
- `src/runner/container/tests.rs` — would inspect containment and residue behavior.
- `src/runner/container/view.rs` — would inspect Git-view materialization paths and cleanup.
- `src/workspace.rs` — would inspect shared capture/diff behavior and path portability.
- `src/workspace_manager.rs` — would inspect Git ref CAS, commit-tree verification helpers, cleanup, and Windows path behavior.
