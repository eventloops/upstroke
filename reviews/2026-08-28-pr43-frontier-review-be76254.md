# PR #43 — frontier review record, `be76254`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, three findings** |
| **Reviewed SHA** | `be76254526574d6b56273556ae8acc7e6343151c` |
| Pull request | eventloops/upstroke#43, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 43`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed record of the first review, the two preserved CI logs, and the pull request's title and body |

The second review of #43. It agreed that 143 has no other construction and that the
success assertion requires exit zero, and then showed the reading still overclaimed:
`_exit(128 + terminating)` names the **path**, not the cause, because
`PENDING_TERMINATION` has several writers — `Supervisor::finish` assigns SIGTERM on a
reaper-cleanup failure with no signal handled at all. It also observed that reaching the
fallback is *expected* in this test, since the helper blocks SIGTERM and the monitor
unblocks only SIGCONT, so `raise` returns zero by design.

Its second finding closed a hole the first review opened: the previous head replaced a
false `CODING_STANDARDS.md` citation with "the recorded practice refuses" re-run
justification, which no document in this repository says — and `reviews/FINDINGS.md` §12
says the opposite for the flake it measured.

## The review, verbatim

## Findings

1. **P1 — the macOS mechanism still overclaims.** No other 143-producing exit was found, and `status.success()` really requires exit zero. But `_exit(143)` proves only that the monitor observed `PENDING_TERMINATION == 15`; it does not prove the SIGTERM handler ran. Cleanup and guard failures independently assign SIGTERM, including `Supervisor::finish` when reaper cleanup fails ([proc.rs](/srv/worktrees/pr43-r2/src/agent/proc.rs:1883)). Concrete sequence: the released worker exits normally → reaper cleanup fails → `PENDING_TERMINATION` becomes 15 → the monitor reaches `_exit(143)` → the outer test produces the identical fingerprint. A later cleanup regression would therefore be classified as “this failure” by the matching rule.

   The record is also internally contradictory: it first establishes that execution passed `raise` and reached the following `_exit`, then says whether `raise` returned is unestablished ([macOS record](/srv/worktrees/pr43-r2/reviews/2026-08-28-macos-proc-signal-single-failure.md:60)). SIGTERM is deliberately blocked before helper exec, while the monitor unblocks only SIGCONT. A blocked signal remains pending, and successful `raise` returns zero, so reaching the fallback is expected here ([Apple `raise(3)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/raise.3.html), [Apple `sigaction(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/sigaction.2.html)). Finally, “every other `_exit` … passes 0 or 1” is literally false: the file has three `_exit(127)` calls.

2. **P1 — the authority correction is incomplete.** The body correctly says `CODING_STANDARDS.md` §12 contains no rerun rule, then later asserts that the same section “forbids ‘it passed on re-run’ as a merge basis” ([pr.md](/srv/worktrees/pr43-r2/pr.md:109)). The section contains no such statement ([CODING_STANDARDS.md](/srv/worktrees/pr43-r2/CODING_STANDARDS.md:402)). Both failure records similarly replace the false citation with an unsupported assertion that “recorded practice refuses” rerun-based justification; `reviews/FINDINGS.md` §12 instead explicitly tells readers to rerun the measured flake ([FINDINGS.md](/srv/worktrees/pr43-r2/reviews/FINDINGS.md:1084)). The ledger nevertheless marks this finding fixed.

3. **P2 — the change silently widened scope.** The exact diff adds three files and 382 lines, including `reviews/2026-08-28-pr43-frontier-review-de901a0.md`; the body repeatedly claims exactly two new files ([pr.md](/srv/worktrees/pr43-r2/pr.md:47)), as does the Windows record. This directly violates the no-silent-scope-widening rule. The rollback history is also stale: the macOS record was added once and modified three times (`02b7399`, `de901a0`, `83d5560`), not twice as claimed. The generated revert command itself correctly expands to all six commits.

The Windows oracle correction, temp-path normalization, `include_str!` audit, nine-column ledger, and all six locations at `de901a0` checked out. No `src/**`, design, decision, dependency, or configuration file changed, so the panic/`anyhow`/path rules are otherwise unaffected.

VERDICT: CHANGES_REQUIRED