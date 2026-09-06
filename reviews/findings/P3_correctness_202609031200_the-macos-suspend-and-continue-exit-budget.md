---
id: PR119-MACOS-PROC-SUSPEND-CONTINUE-EXIT-BUDGET
severity: P3
disposition: deferred
category: correctness
pr: 119
reviewed_sha:
location: src/agent/proc.rs:8202
provenance: undetermined
first_bad:
guard: project owner / the slice that next opens src/agent/proc.rs, once a controlled macOS environment can measure it
---

## Failure sequence

One macOS (`test (macos-latest)`) run failed `agent::proc::tests::terminal_suspend_and_continue_cover_the_isolated_tree` at the **exit wait after `SIGCONT`**, not at the suspension-coverage assertion the test exists for: `wait_for_exit(&mut helper.child, Duration::from_secs(10))` returned `None` and the `expect("continued helper completes normally")` at `src/agent/proc.rs:8202` panicked; `test result: FAILED. 1806 passed; 1 failed; 34 ignored; finished in 242.38s`. The same commit passed the same leg on a rerun in place. No rate has been measured and the test name appears in no earlier record

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens `src/agent/proc.rs`, once a controlled macOS environment can measure it.

**Deferred; nondeterministic on that runner, cause not established.** **The failure.** Run `33798030529`, attempt 1, job `100790356980`, at `4094c57973ffd7f76a9868d634296be8b0f9a3f1` (PR #119). A normal assertion failure with a `failures:` section and a `test result:` line, so not the `PR43-MACOS-PROC-SIGNAL-FINGERPRINT` shape (`exit status: 143`, a different test) and not the C-004 SIGTERM shape. **The rerun.** `gh run rerun 33798030529 --failed` re-ran only the failed job at the byte-identical head: attempt 2, job `100810497995`, `test (macos-latest)` **success**, the test `ok`, `1807 passed; 0 failed; 34 ignored; finished in 288.63s`. One failed and one green attempt at one head prove nondeterminism on that runner and nothing else: not the absence of a defect in the head, and not its origin. That pair is not promoted to a rate. **What failed.** Not the suspension assertion the test exists for, which passed, but the test's own ten-second exit budget after `SIGCONT` (`wait_for_exit(&mut helper.child, Duration::from_secs(10))`, `src/agent/proc.rs:8201` at that head), which spans the helper's `SIGCONT` handler, a monitor thread polling every 10 ms, the worker's 50 ms poll of the `finish` file, the helper's own reap of it, and the helper's exit as a `cargo test` harness. On a `macos-latest` runner whose suite took 242 s, a fixed budget over that chain can expire under load; that is a reading of the code, not a measurement, and the cause is not established. **What addresses it: nothing yet.** Nothing in PR #119 touches `src/agent/`, and PR #125 changes only the forked helpers' startup `READY` waits, not this post-continue exit wait or the shutdown chain behind it; a helper can report `READY` promptly, pass the suspension assertions, and still take over ten seconds to exit. The test's exit budget is owed a load-tolerant fix of its own, by whoever next opens `src/agent/proc.rs`, and a measured rate is what would show whether that fix worked. **The guard is this row.** Whether the tests PR #119 adds to the same executable altered the scheduling this budget depends on is not argued either way.

Filed into `reviews/FINDINGS.md` §2 from the PR #119 sweep as an unexplained macOS observation. The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
