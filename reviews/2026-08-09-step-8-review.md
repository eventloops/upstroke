# Review: Step 8 — event log, resume, status, ledger, cross-process answers

- **Date:** 2026-08-09
- **Scope:** commit `dc56475` (the whole step-8 branch), reviewed in **two
  disjoint passes** — see the coverage split below
- **Level:** ultrareview (cloud multi-agent fleet) over everything except
  `engine.rs`; local `/code-review`-equivalent at max effort over `engine.rs`
- **Result:** 14 reports, 13 distinct defects, **all real, all fixed**. Suite
  250 → 259 tests.

The through-line: **step 8 got the fold right and the seams either side of it
wrong.** The one-fold invariant — `Run::emit` appends and folds through the
same `RunState::apply` that replay uses — drew no findings from either pass,
and `live_state_equals_replayed_state_across_every_ladder_path` held
throughout. Every defect was at a boundary the log does not own: git's commit
versus the log's record of it, a question payload versus the event that closed
it, a sweep's return value versus what it actually applied, a lock versus the
directory it guards. That is the honest shape of an event-sourced design — the
fold is provable, the edges are where the work is.

## Coverage split — read before trusting the numbers

The step-8 diff is 11,289 lines by ultrareview's whole-file count, over its
8,000-line cap. Rather than skip the review, the branch was stacked so the
tool saw everything **except** `engine.rs` (16 files, 3,748 insertions), and
`engine.rs` got its own local max pass. So:

- Every line was reviewed by exactly one pass — no gaps, and no overlap except
  where a finding spanned both halves (#5/#13, found independently from both
  sides).
- The two passes have different guarantees. Ultra compiled and executed the
  crate; the local pass reached its findings by reading, then verified each
  against the code and pinned it with a test.
- Nothing reviewed `engine.rs` and the new modules *together*. The seam
  between them — `engine.rs` calling into `events.rs`/`rundir.rs` — is
  therefore the thinnest coverage in this step, which is exactly where #1 and
  #11 lived.

Process note carried from step 7: the tree was fully committed before launch,
and the scope line in the dialog (3,748 insertions) matched the true diff.
That check worked this time.

## Findings

### Ultrareview — the new modules

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 1 | **`tactus answer <id>` hangs on a terminal.** `prompt_for_answer` used `read_to_string`, which blocks until EOF, not Enter — so the documented common case ("`tactus answer <id>` and then just type") sat there silently after the operator pressed Enter, while the prompt's own legend ("empty aborts") promised Enter-to-submit | normal | CONFIRMED | Branch on `is_terminal()` — the predicate already there to decide whether to print the prompt: `read_line` for a person, `read_to_string` kept for the piped case that may span lines |
| 2 | `resolve_run_id`'s exact-match branch compared case-insensitively and then returned the **uppercased input** instead of the matched entry, so a mixed-case run directory would resolve to a path that opens nothing | nit | CONFIRMED (latent — `ulid()` only emits uppercase) | Return the entry as it exists on disk, matching what the prefix branch already did |
| 3 | `status --follow` gave up after ~2 minutes of silence without asking whether the run was still alive — but a whole attempt (thinking, tools, gates, review) folds into one `attempt_finished`, so a healthy frontier-model run is routinely silent far longer. The live view was dropped mid-run | nit | CONFIRMED | The idle budget now starts only when `is_running` goes false. It was never a timeout on silence; it exists to release a terminal attached to a dead engine |
| 4 | The out-of-range option error rendered the valid range as `1..N` — in Rust, the range that *excludes* N, to the only audience this tool has | nit | CONFIRMED | `1-N`; the test that pinned `"1..2"` moved with it, plus a negative assertion so it cannot drift back |
| 5 | `RunPaths::from_parts(public, public)` — the public path passed in the private slot, contradicting the constructor's contract. Harmless only because liveness reads nothing but the lock file | nit | CONFIRMED (= #13) | Fixed at the callee, as the report argued: `RunLock::acquire` and `is_running` now take the public directory, so the wrong thing can no longer be constructed |

### Local max pass — `engine.rs`

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 6 | **A crash between `git commit` and `task_committed` made the run unresumable without hand surgery.** The window spans three git subprocesses; a kill inside it leaves the branch one commit past its own log, which resume reads as foreign history and refuses — telling the operator to reset away a commit that already passed its gates and its review, and to spend the attempt again | normal | CONFIRMED | Resume adopts *its own* commit: the log must end at a passing attempt for a task that never reached `Done`, the commit must sit directly on the recorded head, and its subject must be the one this engine would have written. Anything less is still refused. §15 updated |
| 7 | **A typed answer was discarded whenever an unrelated answer file arrived during the prompt.** `resolve_one_question` swept after the channel returned and early-returned if the sweep applied *anything* — including an answer to a different question — throwing away words a person had just sat and written, which nothing would ask for again | low | CONFIRMED | Sweep, then still ingest. `ingest_answer`'s open-question guard is what makes both safe: the same question absorbs the duplicate, a different one no longer collaterally drops this reply |
| 8 | Resume ignored the recorded `private_dir` and recomputed it from today's defaults — contradicting `rundir.rs`'s own doc. A resume under a different HOME would scatter the rest of a run's transcripts into a second private root while `status` kept pointing at the first | low | CONFIRMED | Default to the recorded location; an explicit override still wins, for a root that genuinely moved |
| 9 | A hand-written `unanswered` answer file spun the scheduler with no sleep: `sweep_answers` reported `changed` even when `ingest_answer` had declined to apply it, so `drain` looped forever. This voids the loop's own termination argument — that branch is bounded only because it closes the question it fires for | low | CONFIRMED | `ingest_answer` returns whether it applied; only that counts as change |
| 10 | A crash inside `ingest_answer` left the question payload reading as open while the log had already closed it — so `tactus answer` would accept a second answer, report success, and no engine could ever ingest it | low | CONFIRMED | Resume reconciles every payload against the replayed state; the log is authoritative |
| 11 | A failure between `paths.create()` and the first event left a run directory with no `events.jsonl`. Sorting newest, that husk became `latest_run`, so a bare `tactus status` reported "no event log here" — shadowing the real latest run until someone deleted it by hand | low | CONFIRMED | Best-effort cleanup of both halves, so a failure to tidy up cannot mask the error that stopped the run |
| 12 | `interaction.rs` documented `read_answer` as skipping a corrupt file "forever"; it actually propagates the error and stops the run | nit | CONFIRMED | The behaviour is right and the comment was wrong — atomic writes are what *license* strict reading, and silently ignoring what might be an operator's answer is worse than stopping. Comment now says that |
| 13 | Same defect as #5, from the caller's side | nit | CONFIRMED | Fixed once, at the callee |
| 14 | Stale test comment claiming "no adapter for the agent the chain names" — a leftover from an approach the test no longer takes | nit | CONFIRMED | Comment corrected |

## On the tests

Nine tests were added, one per defect that has observable behaviour (#12 and
#14 are comments; #2 and #4 fold into existing tests). The two whose control
flow is subtlest — #3 and #7 — were **verified by mutation**: each fix was
reverted, the test was confirmed to fail, and the fix restored. #7's failure
output reproduced the predicted mechanism exactly (the parked task's question
carrying `answer: None` while its neighbour was released by the file), which
is the difference between a test that passes and a test that proves something.
Step 7 shipped a test that pinned the wrong behaviour; this is the cheap
insurance against repeating that.

#6's test earns particular mention: it runs a task to completion, rewinds the
log to the instant before `task_committed`, and asserts the resume adopts the
commit rather than re-running the work — with a paired negative test that
amends the commit message and confirms the refusal still fires.

#9's test is the one exception to "a regression fails the test": a regression
there **hangs** rather than failing, because the defect is an unbounded spin.
The test comment says so.

## Still open (deliberately, with reasons)

- **Nothing reviewed the `engine.rs`↔`events.rs` seam as a whole**, because
  the diff exceeded ultrareview's cap and had to be split. Two of the three
  normal-severity findings lived on seams. If the free ultra allowance renews,
  that junction is where to point it — not at either half again.
- **`#6`'s adoption is reconciliation, not atomicity.** Git and an append-only
  log cannot be written in one transaction, so the window is inherent; the fix
  recognises the shape the window leaves rather than closing it. The guards
  make a false adoption require someone to hand-craft a commit with the
  engine's exact message for the right task at the right instant.
- **`#11` fires only on a failure between directory creation and the first
  event** — realistically a branch-creation failure. The test induces one with
  a `refs/heads/tactus` D/F conflict, which is portable but narrow; other
  causes of a husk are not covered.
- **~/.tactus accumulated 27 run directories** during the step-8 build session,
  before the private-root test plumbing was complete. Harmless debris, left
  alone rather than swept by a review commit; current tests write only to
  scratch roots, which #8's fix now keeps true across resume.
- **Cross-vendor second opinion and self-review-at-frontier** still need a
  second adapter — step 9, unchanged from step 6's record.
- **Two runs in different run dirs sharing one repo** remains unguarded. The
  advisory lock protects a single run directory; worktrees are v0.2.
