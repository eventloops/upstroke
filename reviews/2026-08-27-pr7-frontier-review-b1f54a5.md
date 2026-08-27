# PR7 frontier review, round 5 — `b1f54a5`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED** |
| Reviewed SHA | `b1f54a59c8174f067bd5566fe03f98f01cae66fc` |
| Previously | `09f9a99`, `bf927f3`, `c2c0294`, `75da796` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort = max`, read-only sandbox |
| Inputs | worktree at the reviewed SHA; `pr.diff` (69 files); `pr.md`; **all four** prior records; the `e7cb2a4..b1f54a5` delta |
| Legs at that SHA | gates clean (1716 + 8); Windows guest 1673 + 10 green first run; CI 10/10 uncancelled first run |

**Five P1s and two P2s. Every one verified here before relaying, and every one is a defect
in the round-4 repairs — the same pattern round 4 had against round 3.**

Round-4 item 5 (the rollback disclosure) is the only one judged closed: *"candid enough — it
identifies the incompatible schema-4 shape, the immediately preceding reader's refusal, and
the absence of shipped schema-4 writers."*

## The one that is not about prose

### 4 (P1). Two dead public methods classified `effect_free` can write an arbitrary path

`WorkspaceManager::commit_identity` and `changed_paths_between` were added for the E6
reconstruction that round 3 **deleted**. They have no callers:

```
$ grep -rn '\bcommit_identity\b|\bchanged_paths_between\b' --include='*.rs' src/ | grep -v '///'
src/workspace_manager.rs:2460:    pub fn commit_identity(...)
src/workspace_manager.rs:2553:    pub fn changed_paths_between(
```

Both are `pub`, `src/lib.rs:54` is `pub mod workspace_manager`, and both are classified
`effect_free` in `effects/wrappers.toml`. Both append a **caller-controlled revision string**
to a `git` argv with no option boundary. Verified empirically in a scratch repository:

```
$ git show -s --no-textconv '--format=%T%n%B' --output=./victim HEAD
$ ls -la ./victim
-rw-rw-r-- 1 ubuntu ubuntu 47 ... ./victim
```

**A method documented as a read, and classified as having no effect, created a file.**
`git diff-tree` accepts `--output=` on the same shape. This is a governed-effect escape and
an undisclosed public-library production effect — measured against the same
downstream-reachability standard the body used to justify narrowing `engine::topology`,
which makes it internally inconsistent as well as wrong.

They are dead, so deletion removes the escape and the classification together.

## The fold's two doors, each enforcing half of what it means

### 1 (P1). `failure.is_none()` does not establish that an attempt succeeded

`check_candidate_prepared` rejects `failure: Some(_)` and **never inspects
`ReviewRecord.outcome`**, though `Failed` and `Unavailable` are authoritative and DESIGN
requires every configured pass to succeed. A `candidate_prepared` with `failure: None` and a
review recording `Failed` is accepted, records the candidate, **charges the allowance**, and
enters `Promoting`.

Worse, the witness I added for round-4 finding 2 asserts its own premise with a fixture
whose reviews are *"only a second-opinion pass"* while the frozen plan for that task requires
the primary pass and no second opinion — so **the positive premise is not a complete
successful attempt either**.

### 2 (P1). The failure door does not require a failure, and my new witness depends on that

`check_attempt_finished` refuses `Succeeded` — and never requires
`finished.record.failure.is_some()`, nor compares the record's attempt to the envelope's:

```
$ sed -n '1914,1990p' src/topology/fold.rs | grep -c 'failure'
0
```

So an in-flight attempt can settle `Closed::Failed { halts_run: true }` carrying a record
that says the work passed. The fold accepts it, charges it, fails the task and halts the run
on a ledger line that reports success.

**And the allowance witness I added in round 4 drives exactly that malformed state.** Its
second-attempt case calls the plain `settle` helper — no failure on the record — while its
comment calls it a *"judged failure"*. A `settle_failing` helper sits immediately beside it,
whose own doc says *"the allowance is decided from `AttemptRecord.failure`"*. So the count
reached 2 for the wrong reason: the witness written to fix a witness-validity defect has one.

The reviewer confirms the architecture around it: *"there are exactly two current calls to
`charge_allowance`, and no third allowance-charging settlement path was found."*

### 3 (P1). "A second pair is unrepresentable" is false

`Probes` exposes `agent()`, `ledger()` and `slots()` independently, and **nothing in the type
system forces `agent()` to use the locks the accessors return.** A conforming implementation
can hold `{used_ledger, reported_ledger}`, run through the first and report the second; P4
leaks in A while P6 reads an empty B and calls the run balanced.

`RunnerProbes` happens to be coherent. The compile-time claim — in the trait doc, in the
test, and in the PR body — is wrong. Moving accessors onto a trait does not couple them.

## The prose, again

### 5 (P1). Five *more* stale comments, including one directly under its own correction

`run.rs:1692` still carries the rejected reinterpretation — *"INV-07 … is about which event
records the candidate, not which settles the attempt: without `attempt_finished(Succeeded)`
the generation never reaches `Promoting`"* — **immediately below the new paragraph that says
the opposite**. I rewrote one paragraph of a doc comment and left the next.

Also `workspace_manager.rs:2447` (recovery "needs" `commit_identity` for the deleted window),
`recover.rs:2372` (recovery appending `candidate_prepared`, and `Spend::replay` deduplicating
— both mechanisms gone), `recover/tests.rs:4478` and `:7321` (a pin-only fixture called
"killed after its settlement"), and `engine/topology.rs:19` saying the P0–P8 typestate is "not
yet written" while `create.rs` implements and documents it.

### 6 (P2). The fifth fabricated identifier, and a count that was wrong

**`TopologyFold::charge_allowance` does not exist.** The method is on `impl RunState`
(`fold.rs:3280`). I wrote that path into the ledger once and the PR body twice — in the round
whose own subject was fabricated identifiers, using a check built to catch them. **The check
does not cover the PR body at all, and I had excluded `reviews/**` from it by design.**

And "all **nine** call sites" of `settle_succeeded` were removed is wrong:
`git grep -n '\.settle_succeeded()' 5ccc8f5^` returns **7**. The removal is complete; the
number is not.

### 7 (P2). `pr.md` still does not describe its own head

The test count is `1712 + 8` — the previous head's; this head is `1716 + 8`, and
`09f9a99..b1f54a5` adds four `#[test]` items. The Windows paragraph claims everything since
`8a163fd` is documentation plus a comment, but its command stops at `327cce3`; against the
real head that range is +1651/−1134 across 17 Rust files. "Fourteen commits behind" is 32.
And the CI guidance contradicts itself — `gh pr checks 31` is called a correct instrument at
lines 160–165 and a wrong one at 256–263.

## Coverage declaration, verbatim

Artifacts and authorities read in full: `DESIGN.md`; `pr.md`; `delta-since-last-review.diff`; `prior-review-1-75da796.md`; `prior-review-2-c2c0294.md`; `prior-review-3-bf927f3.md`; `prior-review-4-09f9a99.md`; `decisions/2026-08-12-merge-queue-execution-topology.md`.

Artifact read in part: `pr.diff` — verified byte-identical to `git diff --no-ext-diff --binary 615597c...b1f54a5`, enumerated all 69 paths and totals, read the entire round-four delta and every cited hunk; I did not linearly read all 58,768 lines.

Changed files read IN FULL:

- `.gitignore`
- `decisions/2026-08-26-durable-retry-feedback.md`
- `decisions/README.md`
- `reviews/2026-08-25-pr7-standards-worklist.md`
- `reviews/2026-08-26-pr7-s5-closing-sweep.md`
- `src/engine/classify.rs`
- `src/engine/mod.rs`
- `src/engine/topology.rs`
- `src/engine/topology/preflight.rs`

Changed files read IN PART:

- `effects/wrappers.toml` — affected wrapper classifications.
- `reviews/2026-08-25-pr7-g2-evidence.md` — probe and settlement claims.
- `reviews/2026-08-25-pr7-standing-questions.md` — E6 and settlement sections.
- `reviews/2026-08-27-pr7-frontier-review-09f9a99.md` — result header and relevant findings.
- `reviews/FINDINGS.md` — approvals, round-four repair appendices, open rows, and claims protocol.
- `src/effects.rs` — production-code blanking and call-census helpers.
- `src/engine/topology/attempt.rs` — attempt/review pipeline contract and seams.
- `src/engine/topology/candidate.rs` — promotion, recovery classification, identity verification, and reclaim.
- `src/engine/topology/create.rs` — `Probes`, `Request`, P4, and append-error accounting.
- `src/engine/topology/create/tests.rs` — probe doubles and new accounting witnesses.
- `src/engine/topology/recover.rs` — E6 removal, promotion completion, and resumed state.
- `src/engine/topology/recover/tests.rs` — recovery ordering, promotion, and orphan-pin witnesses.
- `src/engine/topology/run.rs` — failure settlement and candidate sequence.
- `src/engine/topology/select.rs` — spend replay and selection.
- `src/engine/topology/settle.rs` — failed-settlement construction, allowance semantics, and witnesses.
- `src/events/mod.rs` — `AttemptRecord`, `ReviewRecord`, outcomes, and `FailureRecord`.
- `src/ladder.rs` — allowance decision surface.
- `src/review.rs` — review result/pass semantics.
- `src/runner/container.rs` — Windows `read_racing` area.
- `src/runner/mod.rs` — allowance and call censuses.
- `src/topology/census.rs` — relevant census/identifier snippets.
- `src/topology/events.rs` — attempt settlements, candidate records, and SHA wrappers.
- `src/topology/fold.rs` — all round-four checks/appliers/helpers/witnesses and adjacent candidate logic.
- `src/topology/registry.rs` — frozen review fields and changed initializers.
- `src/workspace.rs` — object-ID validation precedent.
- `src/workspace_manager.rs` — candidate readers, public API, Git arguments, and ref validation.

Changed files NOT READ:

- `reviews/2026-08-24-unfreeze-challenge-request.md` — would check whether approvals actually cover every widened frozen-file change.
- `reviews/2026-08-26-pr7-frontier-review-75da796.md` — would compare the durable transcription with the supplied full prior-review artifact.
- `reviews/2026-08-26-pr7-frontier-review-c2c0294.md` — would compare the durable transcription and carried dispositions with the supplied artifact.
- `reviews/2026-08-27-pr7-frontier-review-bf927f3.md` — would compare the durable transcription and coverage with the supplied artifact.
- `src/capacity.rs` — would inspect pool selection, ceilings, and retry accounting.
- `src/effects/tests.rs` — would inspect parser controls and effect-classification mutation quality.
- `src/engine/assembly.rs` — would inspect selector reachability and shared-plan assembly.
- `src/engine/attempt.rs` — would inspect legacy attempt semantics after shared-authority extraction.
- `src/engine/coordinator.rs` — would inspect legacy wire behavior and shared classifier calls.
- `src/engine/tests.rs` — would inspect facade visibility and legacy byte witnesses.
- `src/engine/topology/attempt/tests.rs` — would inspect whether worker/gate/review tests drive real production shapes.
- `src/engine/topology/dispatch.rs` — would inspect lease, pool, and generation construction.
- `src/engine/topology/dispatch/tests.rs` — would inspect retry/dispatch witness quality.
- `src/engine/topology/emit.rs` — would inspect append-error poisoning and event/fold atomicity.
- `src/engine/topology/emit/tests.rs` — would inspect crash and append-error witnesses.
- `src/engine/topology/identity.rs` — would inspect invocation-ledger and slot invariants.
- `src/engine/topology/preflight/tests.rs` — would inspect resume-side probe-accounting witnesses.
- `src/engine/topology/prelock.rs` — would inspect policy digest and pre-lock authority.
- `src/engine/topology/run/tests.rs` — would inspect live retry/escalation and settlement branch quality.
- `src/engine/topology/scaffold.rs` — would inspect test-only task construction and accidental production reach.
- `src/engine/topology/seams.rs` — would inspect production adapters for duplicated or mismatched state.
- `src/engine/topology/startup.rs` — would inspect lock/census/create ordering.
- `src/engine/topology/startup/tests.rs` — would inspect startup refusal and ownership witnesses.
- `src/events/log.rs` — would inspect append hashing, stable-prefix recovery, and poisoning.
- `src/events/log/tests.rs` — would inspect append-fault and legacy compatibility witnesses.
- `src/gates.rs` — would inspect gate ordering and result classification.
- `src/rundir.rs` — would inspect creator deletion and ownership-proof boundaries.
- `src/runner/container/census/tests.rs` — would inspect the Windows racing failure and census convergence.
- `src/runner/container/exec.rs` — would inspect container execution, accounting, and path handling.
- `src/runner/container/fake.rs` — would inspect pre-clean namespace isolation.
- `src/runner/container/resolve/tests.rs` — would inspect runner reconstruction and policy witnesses.
- `src/runner/container/tests.rs` — would inspect container lifecycle and platform cases.
- `src/runner/container/view.rs` — would inspect Git/container observation paths.
- `src/status.rs` — would inspect how contradictory attempt records are exposed to operators.
