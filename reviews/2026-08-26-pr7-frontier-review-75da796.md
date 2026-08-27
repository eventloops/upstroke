# PR7 frontier review — `75da796`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED** |
| **Reviewed SHA** | `75da796fbb660f9a0926f79b643ae6eac3c03be8` |
| Pull request | eventloops/upstroke#31 |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Driver | `~/bin/review-pr.sh 31` with `REVIEW_DIFF`, `REVIEW_ROOT`, `REVIEW_PROMPT_EXTRA` |
| Inputs | the whole 53,464-line diff, the tree at that sha, `reviews/FINDINGS.md`, the PR body |
| Wall clock | ~57 minutes |
| CI at the reviewed SHA | 9/9 success (run `32999498916`, after a same-sha rerun of one macOS job) |

**Why the diff was supplied rather than fetched.** `gh pr diff 31` returns HTTP 406 — this
pull request is 53,464 diff lines and GitHub's API caps at 20,000. The driver previously
wrote a zero-byte `pr.diff` and would have asked for a verdict on nothing; it now refuses,
and takes the diff by path. `reviews/FINDINGS.md` §22a records that.

**Why the reviewer read adaptively.** 2.42 MB does not fit a context window. Rather than
paste a truncation, the reviewer was given the whole diff, a per-file manifest and the tree
at the sha, told where to spend its reading by risk, and **required to declare its own
coverage**. That declaration is reproduced verbatim below and is the reason a bounded review
can be read honestly.

---

Changes are required. I verified `HEAD` is exactly `75da796fbb660f9a0926f79b643ae6eac3c03be8`; `pr.diff` is byte-identical to the merge-base diff and hashes to `620eed97…9462ca`.

## Findings

1. **The “test-only writer / production effect none” claim is false, and the public surface creates an unrecoverable state.**

The PR claims schema 4 is reachable only through `#[cfg(test)]` code ([pr.md](/srv/worktrees/pr7-frontier-75da796/pr.md:22)). In fact:

- The public library exposes `engine` ([src/lib.rs](/srv/worktrees/pr7-frontier-75da796/src/lib.rs:35)).
- This diff deliberately makes `engine::topology` public while admitting that its promised compile-fail capability fixtures do not exist ([src/engine/mod.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/mod.rs:34)).
- The schema-4 modules are public ([src/engine/topology.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology.rs:41)).
- `prelock::check`, the fully public creation `Request`, `create_run`, `Started::into_handle`, `TopologyRun::resumed`, and `TopologyRun::step` form a non-test writer/driver path ([prelock.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/prelock.rs:155), [create.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/create.rs:1253), [run.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/run.rs:670)).
- Worse, `create_run` documents that the caller must hold a worktree lock and have run the census, but its signature requires neither proof ([create.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/create.rs:1284)).

Concrete harm: a normal downstream executable calls these public APIs and writes P0–P8 schema-4 state; it then exits or crashes. The shipped production recovery path still uses reader ceiling 3 and explicitly refuses every schema-4 log ([recover.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/recover.rs:225)). `upstroke resume` therefore cannot resume state the released library itself created.

The narrower statement that the current `upstroke run` command still uses the legacy coordinator is true. The stronger “reachable only from `#[cfg(test)]`” and `production_effect = none` claims are not. This silently widens scope.

2. **Schema-4 retries violate DESIGN.md by losing failure feedback on crash/resume.**

DESIGN requires gate logs or `required_changes` to return to the same rung and accumulated feedback to reach the next rung ([DESIGN.md](/srv/worktrees/pr7-frontier-75da796/DESIGN.md:313)). It also says only session identity and `resume_next` deliberately fail to survive replay ([DESIGN.md](/srv/worktrees/pr7-frontier-75da796/DESIGN.md:406)).

The implementation expressly makes the feedback brief process-local and recreates it empty on resume ([run.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/run.rs:640)). The durable attempt record copies only `kind`, `origin`, and `reason`, dropping `AttemptFailure.feedback` ([classify.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/classify.rs:188), [events/mod.rs](/srv/worktrees/pr7-frontier-75da796/src/events/mod.rs:731)).

Concrete sequence:

1. Attempt 1 fails a gate with an 8-KB diagnostic tail, or review returns specific `required_changes`.
2. `attempt_finished` is durably appended.
3. The conductor crashes before the next dispatch.
4. Resume rebuilds `brief` as empty.
5. The same-rung retry or escalated rung receives none of the required feedback and can repeat the same defect while consuming another attempt.

The ledger acknowledges this as `PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4` ([FINDINGS.md](/srv/worktrees/pr7-frontier-75da796/reviews/FINDINGS.md:154)), but a ledger disposition cannot waive the sole living authority. This makes “retry and escalation delivered end to end” materially too strong.

3. **A multi-rung exhausted task raises a factually wrong human question, and the tests compose the wrong halves.**

When parking, the driver passes only the current rung’s attempt count and hard-codes `rungs_spent: 1` ([run.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/run.rs:1453), [run.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/run.rs:1725)). Its own comment admits that a multi-rung count remains owed, even though this PR claims the escalation lane is delivered.

Concrete sequence:

1. Use a two-rung chain with `attempts_per = 1`.
2. Rung 0 fails and escalates. The fold resets `attempts_on_rung` to zero for rung 1 ([fold.rs](/srv/worktrees/pr7-frontier-75da796/src/topology/fold.rs:3380)).
3. Rung 1 fails and exhausts the chain.
4. `park_question` receives `attempts_on_rung = 1` and `rungs_spent = 1`.
5. The operator is told “1 attempt(s) across 1 rung(s) all failed,” although two attempts across two rungs failed. That contradicts the shared `ParkSubject` contract ([coordinator.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/coordinator.rs:1459)).

The two-rung test drives both attempts but checks only the escalated model ([recover/tests.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/recover/tests.rs:6075)). The question-count test checks “2 attempt(s)” only on one rung ([recover/tests.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/recover/tests.rs:6307)). The named park-question test uses `Clarify`, whose prose does not print counts. No topology test asserts `rung(s)`.

This is a current witness-validity failure under §23’s own definition.

4. **The frozen-layer scope declaration is materially incomplete.**

The body says there are two frozen-file changes and characterizes the fold-side behavioral change as `TaskFold::defers` ([pr.md](/srv/worktrees/pr7-frontier-75da796/pr.md:41)). But `src/topology/fold.rs` also adds:

- `TaskFold::rung` and `attempts_on_rung`;
- eleven public readers beginning with `ready` ([fold.rs](/srv/worktrees/pr7-frontier-75da796/src/topology/fold.rs:850));
- additional fold behavior and extensive tests.

The exact diff is `+1196/-13` for that file. The ledger’s supposedly corrected measurement still says `+777/-11` ([FINDINGS.md](/srv/worktrees/pr7-frontier-75da796/reviews/FINDINGS.md:151)). Thus the body does not accurately inventory the frozen-layer modification, and the ledger’s own measurement is already stale.

5. **The §22/§23 convergence claim fails its own admissibility definition.**

The current tree contains unversioned false property claims:

- `Settled` says only outage deferral avoids spending allowance and every other settlement spends one; `spends_allowance` also exempts `NeedsHuman`, `NoChain`, `Interrupted`, and `Declined ([settle.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/settle.rs:103), [ladder.rs](/srv/worktrees/pr7-frontier-75da796/src/ladder.rs:236)).
- The same comment says the fold derives allowance from `attempt_started`; it now derives it when applying `attempt_finished`.
- `candidate.rs` still says nothing there is a production path and that its coordinator is “the rest of PR7,” although `TopologyRun` now calls it ([candidate.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/candidate.rs:99)).
- `emit.rs` says invocation-ledger cancellation belongs to the module, while the implementation and later comment say obligation (3) moved to the caller ([emit.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/emit.rs:39), [emit.rs](/srv/worktrees/pr7-frontier-75da796/src/engine/topology/emit.rs:582)).

These are exactly §23’s “unversioned false claim” category ([FINDINGS.md](/srv/worktrees/pr7-frontier-75da796/reviews/FINDINGS.md:1991)). The closing sweep covered only prose added by `d17bcf2` and `4247255`, not the whole PR and not the convergence assertions added at `75da796` ([closing sweep](/srv/worktrees/pr7-frontier-75da796/reviews/2026-08-26-pr7-s5-closing-sweep.md:1)).

The body’s “two things ride open” claim is also false: the same ledger carries at least the feedback-durability and candidate-tree verification defects in addition to Windows racing access and `PR7-R4-LOOP-004`. The scoping therefore defines away findings that satisfy its own stated admissibility test.

6. **Exact-head and flake evidence is overstated.**

The body calls `d17bcf2` the settled head and says a new push invalidates the review ([pr.md](/srv/worktrees/pr7-frontier-75da796/pr.md:226)); the reviewed head is `75da796`. There are six commits after `8e48dd1`, not the asserted two, and the complete CI evidence presented is for `d17bcf2`, not this exact head.

On the carried platform failures:

- The Windows row is reasonably candid about its measurement: 2/4 full-suite failures, 0/3 isolated, one captured `PermissionDenied`, and one presumed cause ([FINDINGS.md](/srv/worktrees/pr7-frontier-75da796/reviews/FINDINGS.md:147)). Its pre-existing attribution is supported. However, it describes a real production retry-bound defect in `read_racing`, not merely a flaky test; calling it a “flake” understates the category.
- The macOS claim is unsupported. “The job failed and the next push cleared” proves neither the cause nor that it was a flake ([pr.md](/srv/worktrees/pr7-frontier-75da796/pr.md:134)). The repository’s measured roughly 1-in-31 process-group flake was measured primarily through Linux full-suite runs and names a specific different test ([FINDINGS.md](/srv/worktrees/pr7-frontier-75da796/reviews/FINDINGS.md:684)). Nothing supplied ties that measurement to the unnamed `cca1276` macOS failure.

## Project-rule check

- Violated: DESIGN’s feedback/replay requirements and the prohibition on silently widening scope.
- No `decisions/` file is modified, so I found no immutable-record edit.
- Within the production regions inspected, I found no added panicking `unwrap`/`expect`, no `anyhow`, and no path construction bypassing `std::path`. This was not an exhaustive read of every changed production file.
- `git diff --check` is clean. I did not run the test suite because this workspace is read-only.

## Coverage declaration

Of the 59 manifest files, I read 10 in full, 23 in part, and did not read 26.

Read in full:

`src/engine/topology/run.rs`, `src/engine/topology/dispatch.rs`, `src/engine/topology/emit.rs`, `src/engine/assembly.rs`, `src/engine/classify.rs`, `src/engine/topology.rs`, `src/ladder.rs`, `src/engine/mod.rs`, `reviews/2026-08-26-pr7-s5-closing-sweep.md`, `.gitignore`.

Also read in full outside the manifest: `DESIGN.md`, `pr.md`, and `pr.manifest`.

Read in part:

`src/engine/topology/recover/tests.rs`, `src/engine/topology/candidate.rs`, `src/engine/topology/recover.rs`, `src/engine/topology/settle.rs`, `src/engine/topology/select.rs`, `src/engine/topology/create.rs`, `src/engine/topology/attempt.rs`, `src/topology/fold.rs`, `reviews/FINDINGS.md`, `src/runner/mod.rs`, `src/engine/topology/prelock.rs`, `src/engine/topology/run/tests.rs`, `src/workspace_manager.rs`, `src/effects.rs`, `src/events/log.rs`, `src/engine/attempt.rs`, `src/review.rs`, `effects/wrappers.toml`, `src/engine/coordinator.rs`, `src/runner/container.rs`, `src/topology/events.rs`, `src/workspace.rs`, `src/events/mod.rs`.

Also read in part outside the manifest: `pr.diff`, `src/lib.rs`, `src/topology/schema.rs`, and `src/agent/proc.rs`. I did not read all 53,464 diff lines linearly; I read the priority production/test hunks and mechanically verified the entire artifact against Git.

Not read:

```text
src/engine/topology/create/tests.rs
src/engine/topology/attempt/tests.rs
src/engine/topology/startup/tests.rs
src/engine/topology/emit/tests.rs
src/engine/topology/scaffold.rs
src/engine/topology/identity.rs
src/engine/topology/startup.rs
src/engine/topology/dispatch/tests.rs
src/events/log/tests.rs
src/engine/topology/seams.rs
src/effects/tests.rs
src/engine/topology/preflight/tests.rs
src/engine/topology/preflight.rs
reviews/2026-08-25-pr7-standing-questions.md
reviews/2026-08-24-unfreeze-challenge-request.md
reviews/2026-08-25-pr7-g2-evidence.md
src/runner/container/census/tests.rs
src/runner/container/fake.rs
src/runner/container/exec.rs
src/rundir.rs
src/runner/container/tests.rs
src/runner/container/resolve/tests.rs
reviews/2026-08-25-pr7-standards-worklist.md
src/engine/tests.rs
src/gates.rs
src/runner/container/view.rs
```

What I would inspect next in the material unread:

- Creation/attempt/startup/dispatch/emit tests and `scaffold.rs`: composed properties, real production-corpus controls, and every append-fault prefix.
- `identity.rs`, `seams.rs`, and preflight: reservation/invocation accounting and cancellation across refused process starts.
- Event-log/effects tests: all five append-error obligations at every appending caller.
- Container files: Windows sharing violations, bounded recovery, cleanup convergence, and cancellation.
- `rundir.rs`: worktree-lock lifetime, ownership proof, and deletion boundaries.
- `gates.rs`: preservation of full gate-tail feedback into retry input.
- `engine/tests.rs`: public-facade and out-of-process reachability controls.
- The unread review documents: approval scope, evidence provenance, and whether frozen-layer exceptions were actually authorized per instance.

VERDICT: CHANGES_REQUIRED