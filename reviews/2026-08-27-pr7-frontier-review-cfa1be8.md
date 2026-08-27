# PR7 frontier review — `cfa1be8`, round 6

Independent adversarial review, `gpt-5.6-sol` at `max` effort, transport `codex exec`,
sandboxed read-only in a worktree checked out at the reviewed head. The diff was
assembled locally against the pull request's own base and verified byte-identical by
the reviewer: `git diff 615597c...cfa1be8`, 70 files, 59,653 lines.

Reviewed head: `cfa1be86aede5a1852d11c5acb57381fffac11cd`.
Inputs: this pull request's diff and body, plus the five prior review records in-tree.

**This is the round the standing stop condition names.** It returned P1s in the doors
and in the probe coupling, so there is no round 7: the three are dispositioned to the
G2 pass and the merge decision goes to the owner. See `reviews/FINDINGS.md` §22e.

---

## Findings

1. **P1 — `candidate_prepared` still accepts a candidate missing configured reviews.** [`AttemptRecord::is_successful`](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/events/mod.rs:660) checks only `failure.is_none()` and `all()` over the reviews that happen to be present. It never compares them with the task’s frozen `FrozenReviews`. A lone passed `second-opinion`, no primary, or even an empty list therefore succeeds. The repaired fixture’s own comment acknowledges that the old lone-second-opinion shape satisfied the predicate ([fold.rs:4556](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/fold.rs:4556)), but only the fixture was repaired.

   Concrete sequence: freeze a task with primary and second-opinion passes; append valid dispatch and attempt-start events; append an otherwise-valid `candidate_prepared` containing only a passed second opinion and `failure: None`. [`check_candidate_prepared`](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/fold.rs:2142) accepts it, then the fold charges the rung, enters `Promoting`/`AwaitingMerge`, and permits `task_candidate_created`. The candidate’s tree was never approved by the required primary reviewer. None of the new witnesses removes a configured pass; they only change an existing outcome.

2. **P1 — the `attempt_finished` repair covers only `Closed`, not `Retained`.** The `Retained` arm checks only the epoch ([fold.rs:1925](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/fold.rs:1925)); both `is_successful` and the envelope/record attempt comparison are inside the `Closed` arm ([fold.rs:1979](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/fold.rs:1979)). `is_failed` has no caller.

   Concrete sequence: start attempt 1, then append a current-epoch `Retained` settlement whose envelope says attempt 1 but whose record says attempt 9, `failure: None`, and all supplied reviews passed. The fold accepts it, charges the allowance, and enters `RetainedIdle`; ready-retry can then run attempt 2 despite the durable record claiming success, while spend/export see attempt 9’s cost and model on attempt 1’s settlement. The in-tree scaffold already demonstrates the missing outcome check by successfully emitting a retained record with `failure: None` and no reviews ([scaffold.rs:1293](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/scaffold.rs:1293)). All new refusal witnesses construct `Closed`, so this arm is undriven.

3. **P1 — two probe-accounting pairs remain representable.** Passing `ledger` and `slots` as trait arguments does not force an implementation to use them. An implementation can own locks A, ignore the supplied locks B, run its `Registering` wrapper through A, and let P6 inspect balanced B. The existing `ContainerProbes` already proves the signature permits ignoring both arguments while running a real shell process ([create/tests.rs:3465](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/create/tests.rs:3465)). The production `RunnerProbes` is coherent, but the claimed signature-level guarantee is false.

   The source also immediately retracts and restates the same claim: “the claim is not restated” is followed by “There is no second pair … a property of the signature” ([create.rs:165](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/create.rs:165)). P6 still has a stale comment saying the probes own the locks although it now reads `Request`’s locks ([create.rs:1762](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/create.rs:1762)).

4. **P2 — the frozen-file scope inventory omits a public API change and includes unused extra scope.** The PR classifies all `src/events/mod.rs +91/−0` as Class C for `FailureRecord::detail` ([pr.md:64](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/pr.md:64)). Thirty of those lines instead add the Class-B success predicate pair. `events` is public, both methods are public, and `is_failed` is unused outside its definition. Thus the diff adds a public method not needed by either door while the stated frozen inventory says the file contains only the optional field work. The numerical total is correct; its semantic classification is not. This violates the rule against silently widening the change’s own scope.

5. **P2 — neither new external gate supports the claims made for it.**

   - `deleted-mechanisms.sh` uses sentence-level joined comments only for its one `CLAIM` regex. Its six named mechanisms use a raw ±3-line window ([deleted-mechanisms.sh:53](/home/ubuntu/tactus-artifacts/pr7/drivers/deleted-mechanisms.sh:53)), contrary to the body’s claim that all seven are checked within their own sentence. Its code scan strips from the first `//`, including one inside a string ([deleted-mechanisms.sh:43](/home/ubuntu/tactus-artifacts/pr7/drivers/deleted-mechanisms.sh:43)). A valid line such as `let _ = "removed //"; settle_succeeded();` is hidden from the code check and passes the tombstone check because `removed` is on the same raw line. The script advertises `--selftest` but implements no argument handling or self-test.
   - `idcheck.sh` still validates qualified paths segment-by-segment ([idcheck.sh:112](/home/ubuntu/tactus-artifacts/pr7/drivers/idcheck.sh:112)). Consequently the exact fabricated `TopologyFold::charge_allowance` passes: `TopologyFold` exists somewhere and `charge_allowance` exists somewhere, though not on that type. The known fabricated path remains in the prior review and the algorithm treats it as resolved before examining context. Moreover, pass 1 on `b1f54a5..cfa1be8` reports `checkpoint_refusals` unresolved, so the gate is not green over the repair range it purports to police.

6. **P2 — repair prose remains false or unsupported.**

   - The body now says `RunState::charge_allowance` “does not exist” because it is declared on `impl RunState` ([pr.md:561](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/pr.md:561)). An inherent method declared in that impl is precisely `RunState::charge_allowance`; the intended historical fabrication was `TopologyFold::charge_allowance`.
   - The repaired helper sets `failure: Some(...)`, while the adjacent comment still says it records `failure: None` ([recover/tests.rs:2049](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/recover/tests.rs:2049)).
   - The same new comment publishes “~8 fixtures” without the command the PR says accompanies every prose count; there are 11 direct call sites to the helper.
   - The Windows paragraph still says everything from `8a163fd` to “this one” is documentation plus one comment, but its command stops at `327cce3` ([pr.md:114](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/pr.md:114)). From `8a163fd` to `cfa1be8` there are 25 commits and 19 Rust files with +2290/−1293 lines.

Repairs I checked and found sound: the successful-candidate allowance now charges live and on replay; the three new `Closed`-settlement refusal cases drive the fold door they name; the original five named stale mechanisms were corrected or deleted; `commit_identity` and `changed_paths_between` and their wrapper entries are gone; the `+1916/−186`, `+91/−0`, and seven-call-site totals re-derive; no existing decision record was edited; and I found no added production panicking `unwrap`/`expect`, non-binary `anyhow`, or non-`std::path` path handling.

I verified HEAD is exactly `cfa1be86aede5a1852d11c5acb57381fffac11cd`, and `pr.diff` is byte-identical to `git diff 615597c...cfa1be8` across 70 files. The filesystem is read-only, so I did not rerun Cargo.

## Coverage

“Read” below means all repair-delta hunks plus the stated relevant/current sections; it does not imply every line of each multi-thousand-line file.

Changed files READ:

- [.gitignore](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/.gitignore) — read the complete histogram-ignore change.
- [decisions/2026-08-26-durable-retry-feedback.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/decisions/2026-08-26-durable-retry-feedback.md) — read fully for authorization, compatibility, and scope.
- [decisions/README.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/decisions/README.md) — read the complete new index entry.
- [effects/wrappers.toml](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/effects/wrappers.toml) — read the affected classifications, including deleted readers and added predicates.
- [reviews/2026-08-27-pr7-frontier-review-b1f54a5.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-27-pr7-frontier-review-b1f54a5.md) — read fully before the repairs.
- [reviews/FINDINGS.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/FINDINGS.md) — read the full repair delta and relevant approval/open-row sections.
- [src/capacity.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/capacity.rs) — read all changed initializers.
- [src/effects.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/effects.rs) — read the production blanker and test-domain sections relevant to rule scans.
- [src/engine/classify.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/classify.rs) — read fully; checked record construction and feedback carriers.
- [src/engine/mod.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/mod.rs) — read the complete visibility change.
- [src/engine/topology.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology.rs) — read the complete repair delta and facade claims.
- [src/engine/topology/candidate.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/candidate.rs) — read the repair delta and promotion/identity-verification path.
- [src/engine/topology/create.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/create.rs) — read the repair delta, `Probes`, P4, and P6 accounting.
- [src/engine/topology/create/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/create/tests.rs) — read the repair delta and probe witnesses/doubles.
- [src/engine/topology/recover.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/recover.rs) — read the repair delta and stable-prefix handoff.
- [src/engine/topology/recover/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/recover/tests.rs) — read the repair delta and affected helpers/witnesses.
- [src/engine/topology/run.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/run.rs) — read the repair delta and settlement/candidate sequence.
- [src/engine/topology/scaffold.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/scaffold.rs) — read retained-attempt and review-plan construction.
- [src/engine/topology/select.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/select.rs) — read the repair delta and checkpoint census.
- [src/engine/topology/settle.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/settle.rs) — read the repair delta and retained/closed settlement construction.
- [src/events/log.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/events/log.rs) — read all changed stable-prefix and hook hunks.
- [src/events/mod.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/events/mod.rs) — read the repair delta and complete `AttemptRecord`/`FailureRecord` areas.
- [src/gates.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/gates.rs) — read the complete shared-command change.
- [src/ladder.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/ladder.rs) — read allowance-shape authority and relevant tests.
- [src/review.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/review.rs) — read frozen pass selection and pass-completeness semantics.
- [src/rundir.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/rundir.rs) — read all changed creation/ownership-proof hunks.
- [src/runner/container.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container.rs) — read changed hooks and the carried Windows retry-bound area.
- [src/runner/container/view.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/view.rs) — read the complete changed test-domain documentation.
- [src/status.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/status.rs) — read all changed initializers.
- [src/topology/census.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/census.rs) — read the repair delta and affected fixtures.
- [src/topology/events.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/events.rs) — read the changed hunks and settlement wire types.
- [src/topology/fold.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/fold.rs) — read the complete repair delta and adjacent checks/appliers/witnesses.
- [src/topology/registry.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/topology/registry.rs) — read changed initializers and frozen review fields.
- [src/workspace.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/workspace.rs) — read the complete shared review-diff change.
- [src/workspace_manager.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/workspace_manager.rs) — read the complete repair deletion and adjacent Git readers.

Changed files DID NOT READ:

- [reviews/2026-08-24-unfreeze-challenge-request.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-24-unfreeze-challenge-request.md) — could reveal an approval narrower than the newly added predicate/API work.
- [reviews/2026-08-25-pr7-g2-evidence.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-25-pr7-g2-evidence.md) — could contain stale probe, recovery, or fault-matrix claims.
- [reviews/2026-08-25-pr7-standards-worklist.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-25-pr7-standards-worklist.md) — could expose misrouted contract findings.
- [reviews/2026-08-25-pr7-standing-questions.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-25-pr7-standing-questions.md) — could retain deleted E6 or settlement mechanisms.
- [reviews/2026-08-26-pr7-frontier-review-75da796.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-26-pr7-frontier-review-75da796.md) — could disagree with the PR body’s historical transcription.
- [reviews/2026-08-26-pr7-frontier-review-c2c0294.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-26-pr7-frontier-review-c2c0294.md) — could reveal a stale disposition or unsupported claimed repair.
- [reviews/2026-08-26-pr7-s5-closing-sweep.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-26-pr7-s5-closing-sweep.md) — could show that the sweep’s actual corpus was narrower than described.
- [reviews/2026-08-27-pr7-frontier-review-09f9a99.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-27-pr7-frontier-review-09f9a99.md) — could disagree with the body’s retained review-evidence section.
- [reviews/2026-08-27-pr7-frontier-review-bf927f3.md](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/reviews/2026-08-27-pr7-frontier-review-bf927f3.md) — could expose another repair premise omitted from this round.
- [src/effects/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/effects/tests.rs) — could reveal weak controls around source blanking or effect classification.
- [src/engine/assembly.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/assembly.rs) — could reveal shared-plan assembly drift.
- [src/engine/attempt.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/attempt.rs) — could reveal legacy semantics changed by the shared authorities.
- [src/engine/coordinator.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/coordinator.rs) — could reveal legacy wire or feedback-carrier regressions.
- [src/engine/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/tests.rs) — could reveal a vacuous legacy-byte or facade witness.
- [src/engine/topology/attempt.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/attempt.rs) — could reveal gate/review ordering or record-construction divergence.
- [src/engine/topology/attempt/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/attempt/tests.rs) — could reveal witnesses constructing outcomes instead of driving the runner.
- [src/engine/topology/dispatch.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/dispatch.rs) — could reveal lease, pool, or generation identity mismatches.
- [src/engine/topology/dispatch/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/dispatch/tests.rs) — could reveal weak retry/dispatch premises.
- [src/engine/topology/emit.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/emit.rs) — could reveal append-error or fold-poisoning inconsistencies.
- [src/engine/topology/emit/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/emit/tests.rs) — could reveal crash witnesses that do not drive the named boundary.
- [src/engine/topology/identity.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/identity.rs) — could reveal invocation-ledger or slot-accounting gaps.
- [src/engine/topology/preflight.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/preflight.rs) — could reveal another runner path bypassing registration.
- [src/engine/topology/preflight/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/preflight/tests.rs) — could reveal vacuous resume-side accounting witnesses.
- [src/engine/topology/prelock.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/prelock.rs) — could reveal policy-digest authority drift.
- [src/engine/topology/run/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/run/tests.rs) — could reveal live retry/escalation tests that bypass production settlement construction.
- [src/engine/topology/seams.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/seams.rs) — could reveal duplicated or mismatched shared state.
- [src/engine/topology/startup.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/startup.rs) — could reveal incorrect census/lock/create ordering.
- [src/engine/topology/startup/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/engine/topology/startup/tests.rs) — could reveal ownership/refusal witness defects.
- [src/events/log/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/events/log/tests.rs) — could reveal weak stable-prefix and torn-write controls.
- [src/runner/container/census/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/census/tests.rs) — could refine or contradict the reported Windows race evidence.
- [src/runner/container/exec.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/exec.rs) — could reveal container accounting, platform, or path defects.
- [src/runner/container/fake.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/fake.rs) — could reveal pre-clean namespace interference.
- [src/runner/container/resolve/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/resolve/tests.rs) — could reveal weak policy-reconstruction witnesses.
- [src/runner/container/tests.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/container/tests.rs) — could reveal lifecycle or platform-specific regressions.
- [src/runner/mod.rs](/home/ubuntu/.claude/jobs/5a783327/tmp/r6/root/src/runner/mod.rs) — could reveal a census or process-start bypass outside the targeted scans.

VERDICT: CHANGES_REQUIRED