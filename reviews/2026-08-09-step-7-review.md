# Review: Step 7 — retry/escalation ladder, questions, parking scheduler

- **Date:** 2026-08-09
- **Scope:** the step-7 working diff (src/engine.rs rewritten, src/ladder.rs and
  src/interaction.rs new, src/ir.rs, src/config.rs, src/review.rs, src/gates.rs,
  src/main.rs, src/lib.rs)
- **Level:** ultrareview (cloud multi-agent fleet) — **with an incomplete scope, see below**
- **Result:** 5 reported, 4 real; all fixed. Suite 186 → 188 tests.

The through-line: **the ladder was right; the classification feeding it was wrong.**
Step 7's whole premise is that `FailureKind` dispatch keeps an outage from being
punished as a code rejection — and the highest-severity finding was a control-flow
ordering bug that silently converted rate limits and timeouts into parked questions,
defeating exactly that rule. The pure decision function it feeds had no defects.

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 1 | `evaluate_outcome` scanned for the `TACTUS-QUESTION:` marker **before** matching on `OutcomeStatus`, so any Timeout / RateLimited / AgentError whose `detail` merely contained the marker was reclassified `NeedsHuman` — defeating "RateLimited defers rather than burning an attempt" (§19) and discarding the timeout's transcript-tail feedback | normal | CONFIRMED | Marker check moved inside the `Completed` arm, where the comment always said it belonged; regression test asserts all three failure statuses keep their own kind |
| 2 | `resolve_one_question` was not guarded by `halted_at.is_none()` the way the other two `drain` branches are: after a halt, an operator's answer was written to disk and then silently ignored (task reported `Skipped`), and a decline routed through `fail_task` and relabelled `halted_at` with the wrong task | normal | CONFIRMED | Branch guarded, symmetric with the others; `fail_task` now uses `get_or_insert_with` so the first failure keeps the label. Questions stay open on disk for a later resume |
| 3 | `worker_question` used `find` (first occurrence) while its own doc comment and the prompt both specify *last* — and the engine's own empty-diff feedback names the marker verbatim, so an echo is the expected case | nit | CONFIRMED | `find` → `rfind`, matching `review.rs`'s established last-wins rule for verdicts; test covers an echoed marker preceding the real question |
| 4 | `fence_for` duplicated byte-for-byte in `engine.rs` and `review.rs` — a prompt-injection defence whose invariant would have to be maintained in two places | nit | CONFIRMED | Hoisted to `util.rs` beside the other string helpers; both copies deleted |
| 5 | "Missing `src/interaction.rs` and `src/ladder.rs` — crate does not compile" | normal | **NOT A DEFECT** | Scope artifact: both files were untracked at launch, so they were absent from the uploaded bundle. They exist and the crate builds |

## Scope caveat — read before trusting the coverage

Ultrareview's scope is *branch vs default branch plus uncommitted and staged
changes*. Untracked files are neither, so the two new modules were **not in the
bundle**. Two consequences:

- `ladder.rs` and `interaction.rs` received **no review at all** (~870 lines).
- More importantly, the crate **could not be compiled** in the sandbox, so no test
  ran and no behaviour was observed. Every finding was reached by reading, not by
  execution — a weaker guarantee than the "independently reproduced and verified"
  the feature advertises. The four real findings all held up on inspection, so the
  reading was sound; the guarantee simply wasn't the one on the tin.

**Process fix: `git add -N` (or commit) new files before launching a review.**
The scope line in the launch dialog is the check — it reported 2,783 insertions
where the true diff was 3,665.

## Still open (deliberately, with reasons)

- **`ladder.rs` and `interaction.rs` remain unreviewed by anything but their own
  tests.** A local `/code-review` over the working diff covers them without
  spending an ultrareview run.
- **`fail_task`'s first-failure-wins guard is belt-and-braces.** With the
  `drain` branch now guarded, a second `fail_task` after a halt is unreachable,
  so the `get_or_insert_with` is defensive rather than exercised. Kept because it
  is free and makes the invariant local rather than emergent.
- **Blocked-propagation, the drain-loop termination argument, and the
  keep-the-tree-on-resume invariant** were flagged in advance as the thinnest
  parts of the change. The review reached none of them — absence of findings there
  is not evidence of correctness, since nothing was executed.
