# PR7 frontier re-review — `c2c0294`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED** |
| Reviewed SHA | `c2c029445a3956a713bd692f72d580e715f7ad1c` |
| Previously reviewed | `75da796` ([record](2026-08-26-pr7-frontier-review-75da796.md), CHANGES_REQUIRED) |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort = max`, read-only sandbox |
| Started / saved | 2026-08-26 21:56 / 22:21 UTC |
| Inputs | a worktree checked out at the reviewed SHA; `pr.diff`; `pr.md`; `prior-review-75da796.md`; `delta-since-reviewed-head.diff` (`610106b..c2c0294`) |
| CI at that SHA | 10/10 success, uncancelled — run `33016262289` |
| Driver | `~/bin/review-pr.sh` with `REVIEW_DIFF`, `REVIEW_ROOT`, `REVIEW_PROMPT_EXTRA` |

## The one thing to read before the findings: the diff supplied was against the wrong base

`gh pr view 31 --json baseRefName` is **`codex/parallelism-design`**. The diff handed to the
reviewer was assembled against `git merge-base master c2c0294` = `13c5d0a`. The difference
is not cosmetic:

```
supplied (master base)      : 182 files changed, 189096 insertions(+), 16679 deletions(-)
correct  (integration base) :  65 files changed,  52548 insertions(+),   577 deletions(-)
```

The extra 117 files are the integration branch's own already-merged history — the repository
rename, the frontier-workflow retirement authorised by
`decisions/2026-08-23-retire-app-attestation.md`, and slices PR3 through PR6. **Finding D
below is entirely an artifact of this**, and it is a defect in the review driver rather than
in the pull request. Last round's rider made `review-pr.sh` refuse an empty diff; it does not
check that a supplied diff is *the right change*, which is the same class one turn along.
The reviewer also spent budget reading 117 files of noise, which bears on the coverage
declaration.

Every other finding was re-derived here against the tree itself and is unaffected.

## Disposition of the six prior findings, as the reviewer states them

| # | reviewer's verdict | verified here |
|---|---|---|
| 1 pub surface | **closed** for the original defect; the broader `production_effect = none` claim is independently false because of A | confirmed both halves |
| 2 feedback durability | original **closed**; the repair introduces a new defect (A) | confirmed both halves |
| 3 `rungs_spent` | **closed** for the reported sequence; its doc's stronger claim is false for duplicate-tier chains | confirmed both halves |
| 4 frozen-layer inventory | **not closed** — the table mixes comparison domains | confirmed |
| 5 convergence claim | **not closed** — false property claims remain, and the body still declares convergence | confirmed |
| 6 exact-head and platform evidence | **not closed** — three stale claims remain | confirmed |

## Blocking findings, each re-derived

### A. The feedback repair reaches the legacy schema-3 wire and `report.json` — CONFIRMED

The Class C authorization scoped the field to schema 4. It does not stay there, because the
record builder is shared:

```
$ grep -n 'classify::attempt_record' src/engine/coordinator.rs
844:                data: Box::new(super::classify::attempt_record(
                        failure: result.failure.as_ref(),

$ sed -n '206p' src/engine/classify.rs
            detail: failure.feedback.clone(),

$ grep -n 'pub attempts' src/engine/report.rs
83:    pub attempts: Vec<AttemptRecord>,
530:        attempts: records.clone(),
```

`coordinator.rs` is the **live** schema-3 path — the one `upstroke run` uses today. It passes
the same `AttemptFailure` into the shared builder, so an 8-KiB gate tail or an uncapped
`required_changes` is now written into `FailureRecord.detail` on the schema-3 wire, in
addition to the `LadderRetry`/`LadderEscalated` copy that already carried it, and from there
into `report.json` once per failed attempt.

**It reverses a decision the codebase states outright.** `LadderRetry`'s own doc: *"Carried on
the ladder events rather than on the attempt record because this is the full text — a gate
log tail runs to kilobytes, and `report.json` should not grow one per attempt."* That is
precisely what now happens.

**And it falsifies three claims made in support of the change**: the decision record's
"`report.json` is unaffected"; the body's `production_effect = none`; and the body's rollback
statement that no serialization changed. The record's reasoning — "the legacy path constructs
its feedback into `LadderRetry` as before and this change adds no call site to it" — was
asserted without measuring the caller, in the record whose whole subject is that property
claims must be measured.

**Repair, and it needs no new authorization** because it restores the authorised scope: the
caller decides. `AttemptFacts` gains an explicit choice, the schema-4 driver passes the
feedback and the legacy coordinator passes `None`, with a witness on each side. The
alternative — accepting the schema-3 change — is a widening of the Class C exception and is
the owner's to grant, not mine to assume.

### B. Recovery still does not verify the judged candidate tree — CONFIRMED as an open row

`DESIGN.md:410` requires `candidate_prepared` to record "exactly one complete
attempt/base/commit/tree identity ... so resume adopts only that exact shape". The event
carries `tree_sha`; `topology::fold::PreparedCandidate` drops it, so recovery checks object
existence and parent only — and `candidate.rs`'s own comment admits a same-parent,
different-tree commit passes.

This is `PR7-CANDIDATE-TREE-UNVERIFIED` in `reviews/FINDINGS.md` §2, carried by disposition.
**The reviewer's argument is the one that carried finding 2 and it was accepted then**: a
ledger disposition records a decision, it does not amend the sole living authority. The
repair is a field on a frozen fold type, which is the same Class C shape F2 was — so this is
a stop-and-ask, not an implementer's call.

### C. Five false property claims, four of them in code this round touched — CONFIRMED

| where | says | is |
|---|---|---|
| `src/engine/topology/settle.rs:109` | "**Five** kinds spend nothing — `NeedsHuman`, `NoChain`, `Interrupted`, `Declined` and the outage deferral" | **seven shapes**. `spends_allowance` returns false for those four kinds *and*, via `FailureShape::is_outage`, for `RateLimited` (any origin), `ReviewUnavailable` (any origin), and `Timeout` with `FailureOrigin::Reviewer`. Calling the outage arm "the outage deferral" hides three. **Third restatement of this sentence and still wrong** — round 6 corrected "one" to "five" and the answer was never five. |
| `src/events/mod.rs:751` | `attempt_record` is "the one production construction of an `AttemptRecord`" | two: `events::Dangling::event` is the other. `classify.rs`'s copy of this sentence was corrected in the same commit — **and a fresh copy was written here**, in the new field's doc. One copy fixed, one copy created. |
| `src/engine/topology/recover/tests.rs:6679` | after an old log, "the brief is simply empty for the attempts that predate the field" | `Brief::record` adds an entry whenever `record.failure` is `Some`, carrying the summary with `detail: None`. The brief is not empty; the test only asserts the resume succeeds. |
| `src/engine/topology/recover.rs:1868` | "PR7 implements neither terminal" (`task_candidate_created` or a CAS) | `finish_promotions` calls `candidate::append_candidate_created` at `recover.rs:2207`. |
| `src/engine/topology/run.rs:1845` | `task.rung + 1` "matches the legacy authority's rule", which counts **distinct tiers** | `chain: Vec<Tier>` is not deduplicated, so `chain = ["small", "small"]` gives topology 2 and legacy 1. The claim holds only for chains with no repeated tier, and nothing enforces that. |

None carries a command and result, which is what §19's rule requires of exactly this kind of
sentence — and two were written this round, in the commit that repaired the previous batch.

### D. "The exact diff is much larger and rewrites immutable records" — DISMISSED, harness defect

The six dated decision records the reviewer names are **not touched by this pull request**:

```
$ git diff --stat 615597c...c2c0294 -- decisions/2026-08-11-*.md decisions/2026-08-12-*.md
(no output)
```

They differ only in the wrongly-based diff, where the repository rename on the integration
branch supplies the 34 insertions and 34 deletions. Same for the deleted frontier workflows,
which `decisions/2026-08-23-retire-app-attestation.md` authorised and which merged before this
branch opened. The finding is correct about what it was shown and wrong about the pull
request; the fault is the input.

## Findings 4, 5 and 6, re-derived

**4 — the frozen-layer table mixes two comparison domains.** Its header says "derived from the
staged tree at `c2c0294`", and it is not one derivation:

```
$ git diff 615597c...c2c0294 --numstat -- src/events/mod.rs src/topology/events.rs src/topology/fold.rs src/topology/registry.rs
48      0       src/events/mod.rs
30      1       src/topology/events.rs
1197    13      src/topology/fold.rs
2       0       src/topology/registry.rs
```

The `fold.rs` row was the slice total measured at `2378c83` (**+1196/−13**, now **+1197/−13**
— F2 added a fixture line to it); the other three rows were the F2 commit's own deltas
(`75da796..c2c0294`). One table, two domains, one header claiming a single derivation.

**5 — the body still declares convergence.** Line 249 reads "**S5 IS CONVERGED, and the word
is scoped**" while the repair table 100 lines below says the claim was withdrawn as a merge
claim. `reviews/FINDINGS.md` §23 was narrowed; the body was not.

**6 — three stale claims.** "the two commits since `8e48dd1`" (four through `d17bcf2`, eleven
through this head); "the branch has one macOS flake on record" against the ledger's 12
success / 2 failure over 14 completed jobs; and the retired `PR7-WIN-RACING-ACCESS-ESCAPES`
ID, which was corrected in the body **after** the reviewer fetched it and is no longer
present.

## What the reviewer could not do

- **It did not run the suite.** `cargo` failed with `sccache: Operation not permitted`, and
  with `RUSTC_WRAPPER` removed it could not create a target path because the checkout is
  read-only. Every finding above is from reading, and every one of them was re-derived here
  against a writable tree.
- `CODING_STANDARDS.md` is absent from the tree, so it routed no conformance-only finding.
- `git diff --check` was clean; it found no newly added production `unwrap`/`expect`,
  `anyhow`, or non-`std::path` bypass in the paths it inspected.

## Coverage declaration, verbatim

Reproduced exactly as the reviewer wrote it. The unread set is large partly because the
supplied diff carried 117 files that are not this pull request's — see the note at the top.

The coverage universe is all 182 paths in the supplied exact `pr.diff`, plus the four supplied review artifacts.

### Read in full

- `DESIGN.md`
- `decisions/2026-08-21-stacked-slice-prs.md`
- `decisions/2026-08-26-durable-retry-feedback.md`
- `decisions/README.md`
- `src/engine/attempt.rs`
- `src/engine/classify.rs`
- `src/engine/topology/emit.rs`
- `src/engine/topology/run.rs`
- `src/lib.rs`
- `pr.md`
- `prior-review-75da796.md`

### Read in part

- `delta-since-reviewed-head.diff`
- `pr.diff`
- `MAINTAINING.md`
- `decisions/2026-08-11-codex-reasoning-effort.md`
- `decisions/2026-08-11-design-council.md`
- `decisions/2026-08-11-export-decisions-schema.md`
- `decisions/2026-08-11-resume-gate-config.md`
- `decisions/2026-08-11-self-hosting-v02.md`
- `decisions/2026-08-12-merge-queue-execution-topology.md`
- `reviews/2026-08-26-pr7-frontier-review-75da796.md`
- `reviews/FINDINGS.md`
- `src/capacity.rs`
- `src/config.rs`
- `src/effects.rs`
- `src/engine/assembly.rs`
- `src/engine/coordinator.rs`
- `src/engine/mod.rs`
- `src/engine/report.rs`
- `src/engine/tests.rs`
- `src/engine/topology.rs`
- `src/engine/topology/attempt.rs`
- `src/engine/topology/candidate.rs`
- `src/engine/topology/recover.rs`
- `src/engine/topology/recover/tests.rs`
- `src/engine/topology/seams.rs`
- `src/engine/topology/select.rs`
- `src/engine/topology/settle.rs`
- `src/engine/topology/startup.rs`
- `src/engine/topology/startup/tests.rs`
- `src/events/log.rs`
- `src/events/mod.rs`
- `src/export.rs`
- `src/gates.rs`
- `src/ladder.rs`
- `src/main.rs`
- `src/route.rs`
- `src/status.rs`
- `src/topology/events.rs`
- `src/topology/fold.rs`
- `src/topology/registry.rs`
- `src/workspace.rs`
- `src/workspace_manager.rs`

### Not read

- `.git-blame-ignore-revs` — would check whether evidence-bearing commits are hidden from blame.
- `.github/pull_request_template.md` — would check whether the template permits the scope/evidence contradictions found here.
- `.github/scripts/frontier-check-payload.sh` — would inspect what frontier-verdict validation its deletion removes.
- `.github/scripts/frontier-invalidation-plan.sh` — would inspect invalidation behavior lost by deletion.
- `.github/scripts/invalidate-frontier-check.sh` — would inspect whether deleting it weakens stale-review protection.
- `.github/scripts/test-frontier-check.sh` — would inspect tests removed with the frontier gate.
- `.github/scripts/test-frontier-evidence.sh` — would inspect whether evidence validation loses coverage.
- `.github/scripts/test-frontier-invalidation.sh` — would inspect invalidation regression witnesses.
- `.github/scripts/test-frontier-workflow.sh` — would inspect workflow-policy assertions removed.
- `.github/scripts/test-pr-ledger-evidence.sh` — would check whether ledger-evidence claims are actually enforced.
- `.github/scripts/test-release-record.sh` — would check release-evidence witness quality.
- `.github/scripts/validate-frontier-evidence.sh` — would inspect validation functionality removed.
- `.github/scripts/validate-release-record.sh` — would check schema and fail-closed behavior.
- `.github/workflows/ci.yml` — would inspect platform coverage, cancellation, and required-gate aggregation.
- `.github/workflows/frontier-review-invalidate.yml` — would inspect the exact protection removed.
- `.github/workflows/frontier-review.yml` — would inspect reviewer trust and scope enforcement removed.
- `.github/workflows/pr-policy.yml` — would inspect whether immutable decisions and exact scope are enforced.
- `.github/workflows/release.yml` — would inspect release provenance and rollback implications.
- `.gitignore` — would check whether new durable or sensitive artifacts can be committed accidentally.
- `CHANGELOG.md` — would check disclosure of production wire/report changes.
- `CLAUDE.md` — would inspect agent instructions for stale or broadened authority.
- `CONTRIBUTING.md` — would check repository rules affected by the exact pile.
- `Cargo.lock` — would inspect dependency and MSRV drift.
- `Cargo.toml` — would inspect features, targets, and dependency scope.
- `KICKOFF.md` — would inspect stale scope and project-name claims.
- `README.md` — would inspect user-facing behavior claims affected by schema/report changes.
- `acceptance/README.md` — would inspect acceptance scope and current commands.
- `acceptance/RESULT-2026-08-11.md` — would inspect whether historical evidence was rewritten.
- `acceptance/RESULT.md` — would inspect current acceptance claims.
- `acceptance/plan.md` — would inspect task scope and acceptance coverage.
- `acceptance/upstroke.toml` — would inspect platform paths, gates, and routing.
- `clippy.toml` — would inspect newly weakened or widened lint policy.
- `decisions/2026-08-17-review-effort-and-fan-out.md` — would inspect review-evidence obligations.
- `decisions/2026-08-20-automated-review-gate.md` — would inspect whether deleted workflows contradict its history.
- `decisions/2026-08-20-review-invalidation-scope.md` — would inspect exact-head invalidation requirements.
- `decisions/2026-08-22-strategy-record-private.md` — would inspect scope boundaries.
- `decisions/2026-08-23-retire-app-attestation.md` — would inspect authorization for frontier-workflow deletion.
- `docs/index.html` — would inspect user-visible stale names and behavior.
- `effect_sites.json` — would inspect missing or broadened effect sites.
- `effects/allowlist.toml` — would inspect new effect exemptions and frozen-layer scope.
- `effects/funnel-modules.json` — would inspect funnel omissions.
- `effects/residue-classes.json` — would inspect crash-residue classification completeness.
- `effects/wrappers.toml` — would inspect wrapper bypasses.
- `examples/probe.rs` — would inspect public API reachability and path portability.
- `fixtures/cyclic-plan.md` — would inspect cycle witness relevance.
- `fixtures/sample-plan.md` — would inspect whether example plans reach changed behavior.
- `proposals/2026-08-13-v0.2-implementation-checkpoints.md` — would compare proposed checkpoints with current refusals.
- `proposals/2026-08-13-v0.2-review-finding-telemetry.md` — would inspect report/wire expectations.
- `proposals/2026-08-13-v0.2-upstroke-commit-provenance.md` — would inspect candidate identity expectations.
- `proposals/2026-08-13-v0.3-public-run-viewer.md` — would inspect implications of exposing `FailureRecord.detail`.
- `proposals/2026-08-13-v0.5-portfolio-coordination-critique-claude.md` — would inspect inherited constraints.
- `proposals/2026-08-13-v0.5-portfolio-coordination.md` — would inspect future scheduling assumptions.
- `proposals/2026-08-15-v0.2-pr-rationale-and-acceptance-traceability.md` — would inspect PR scope/traceability requirements.
- `proposals/2026-08-15-v0.2-review-convergence-and-defect-governance.md` — would inspect the meaning of convergence.
- `proposals/2026-08-15-v0.2-streamed-agent-supervision.md` — would inspect process and feedback assumptions.
- `proposals/2026-08-15-v0.3-blind-normalized-design-council.md` — would inspect reviewer-independence assumptions.
- `proposals/2026-08-15-v0.3-proposal-disposition-ledger.md` — would inspect immutable-history rules.
- `proposals/2026-08-22-v0.3-hazard-map.md` — would inspect whether candidate/report hazards are acknowledged.
- `proposals/README.md` — would inspect proposal-versus-authority boundaries.
- `reviews/2026-08-09-step-10-review-pass-a.md` — would inspect historical findings altered by the exact pile.
- `reviews/2026-08-09-step-10-review-pass-b.md` — would inspect historical independent evidence.
- `reviews/2026-08-09-step-7-review.md` — would inspect inherited open findings.
- `reviews/2026-08-09-step-8-review.md` — would inspect inherited event/wire findings.
- `reviews/2026-08-09-step-9-review-pass-a.md` — would inspect inherited review-pipeline findings.
- `reviews/2026-08-09-steps-1-4.md` — would inspect early assumptions now relied upon.
- `reviews/2026-08-24-unfreeze-challenge-request.md` — would inspect authorization for frozen-file changes.
- `reviews/2026-08-25-pr7-g2-evidence.md` — would inspect whether claimed 117/117 evidence covers the defects found.
- `reviews/2026-08-25-pr7-standards-worklist.md` — would inspect only nonblocking standards routing.
- `reviews/2026-08-25-pr7-standing-questions.md` — would inspect whether known contract defects were omitted.
- `reviews/2026-08-26-pr7-s5-closing-sweep.md` — would inspect the exact limits of the claimed sweep.
- `reviews/README.md` — would inspect durable-review record rules.
- `scripts/rename-tactus-to-upstroke.sh` — would inspect why immutable records were rewritten.
- `src/agent/bin.rs` — would inspect process spawning, platform paths, and panic-free error handling.
- `src/agent/claude.rs` — would inspect command construction and feedback handling.
- `src/agent/codex.rs` — would inspect command construction, effort, and Windows portability.
- `src/agent/copilot.rs` — would inspect sessionless retry and feedback behavior.
- `src/agent/mod.rs` — would inspect adapter facade widening.
- `src/agent/proc.rs` — would inspect process containment and Windows-specific behavior.
- `src/answer.rs` — would inspect answer-ingestion scope accidentally included.
- `src/catalog.rs` — would inspect model/binding drift.
- `src/connect.rs` — would inspect external-effect and path handling.
- `src/effects/tests.rs` — would inspect whether effect censuses fail closed.
- `src/engine.rs` — would inspect behavior lost or changed in its deletion/split.
- `src/engine/options.rs` — would inspect binary-edge option propagation.
- `src/engine/preflight.rs` — would inspect legacy preflight invariants.
- `src/engine/resume.rs` — would inspect legacy recovery compatibility with the new field.
- `src/engine/topology/attempt/tests.rs` — would inspect verification-ladder witness quality.
- `src/engine/topology/create.rs` — would inspect schema-4 creation authorization and crash prefixes.
- `src/engine/topology/create/tests.rs` — would inspect creation witness validity.
- `src/engine/topology/dispatch.rs` — would inspect reservation and attempt-start ordering.
- `src/engine/topology/dispatch/tests.rs` — would inspect dispatch mutation coverage.
- `src/engine/topology/emit/tests.rs` — would inspect append-error obligations and leaking fixtures.
- `src/engine/topology/identity.rs` — would inspect reservation/invocation identity invariants.
- `src/engine/topology/preflight.rs` — would inspect topology runner certification.
- `src/engine/topology/preflight/tests.rs` — would inspect refusal witnesses.
- `src/engine/topology/prelock.rs` — would inspect path and lock ordering.
- `src/engine/topology/run/tests.rs` — would inspect driver branch witnesses not located in recovery tests.
- `src/engine/topology/scaffold.rs` — would inspect test-only callers satisfying production censuses.
- `src/error.rs` — would inspect typed-error coverage and accidental `anyhow`.
- `src/events/log/premove.rs` — would inspect pre-move crash handling.
- `src/events/log/tests.rs` — would inspect strict parsing and append-error witnesses.
- `src/interaction.rs` — would inspect answer/question behavior outside PR7 scope.
- `src/ir.rs` — would inspect shared wire/IR changes.
- `src/plan/markdown.rs` — would inspect plan-authored path and identifier sanitization.
- `src/plan/mod.rs` — would inspect plan parsing and scope changes.
- `src/review.rs` — would inspect reviewer feedback bounds and required-changes propagation.
- `src/rundir.rs` — would inspect public/private path construction and Windows behavior.
- `src/runner/container.rs` — would inspect the known Windows retry-bound defect.
- `src/runner/container/census.rs` — would inspect concurrent census correctness.
- `src/runner/container/census/tests.rs` — would inspect whether the Windows race witnesses kill the defect.
- `src/runner/container/env.rs` — would inspect environment leakage.
- `src/runner/container/exec.rs` — would inspect command and effect containment.
- `src/runner/container/fake.rs` — would inspect fixture isolation and pre-clean safety.
- `src/runner/container/intent.rs` — would inspect durable intent/crash behavior.
- `src/runner/container/resolve.rs` — would inspect image/runtime resolution.
- `src/runner/container/resolve/tests.rs` — would inspect resolution witnesses.
- `src/runner/container/runtime.rs` — would inspect external runtime error classification.
- `src/runner/container/tests.rs` — would inspect containment and cleanup coverage.
- `src/runner/container/view.rs` — would inspect status projection and path handling.
- `src/runner/host.rs` — would inspect the recorded macOS containment race.
- `src/runner/invocation.rs` — would inspect invocation registration/cancellation invariants.
- `src/runner/mod.rs` — would inspect Runner contract widening.
- `src/runner/policy.rs` — would inspect host/container policy boundaries.
- `src/topology/census.rs` — would inspect all fold readers and census completeness.
- `src/topology/effects.rs` — would inspect topology effect classification.
- `src/topology/leases.rs` — would inspect lease-release correctness.
- `src/topology/mod.rs` — would inspect public topology vocabulary surface.
- `src/topology/paths.rs` — would inspect cross-platform path containment.
- `src/topology/queue.rs` — would inspect candidate FIFO and integration readiness.
- `src/topology/schema.rs` — would inspect schema-3/4 reader-ceiling and compatibility rules.
- `src/util.rs` — would inspect identifier/path sanitization and panic risks.
- `src/validate.rs` — would inspect validation scope and typed errors.
- `upstroke.toml` — would inspect gates, routing, duplicate tiers, and blast-radius policy.
