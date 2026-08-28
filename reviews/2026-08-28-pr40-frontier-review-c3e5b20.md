# PR #40 — frontier review record, `c3e5b20`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, six findings** |
| **Reviewed SHA** | `c3e5b20a2b8db9ffd8bb59684cd225bdefe43c2f` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, after a re-run of one macOS leg; attempt 1 is recorded in pull request #43 |
| Inputs | the diff, the tree at that sha, and the four prior review records |

The fourth review of #40, and the first to see the committed review records and the
Review history section as content in their own right. Two of its findings correct
statements this seat had made that were simply false — the exit-143 mechanism and a
leg count — and one is the enumeration tool failing the class it was built to catch.

## The review, verbatim

1. **P1 — the rollback command includes unrelated merged history.** [pr.md](/srv/worktrees/pr40-rereview3/pr.md:348) uses `git log --no-merges 5763fe3..HEAD`. Because `55d50ae` merged the integration branch, that range produces 20 commits: the intended eight first-parent repairs plus 12 commits from the merged side, including `d6e7eea`, `dd961c7`, and source fixes. An operator runs the command, reverses the recent documentation repairs, then begins reversing unrelated bridge changes or stops with a partially applied conflict. It cannot produce the promised `5763fe3` document state. Adding `--first-parent` yields the intended eight commits.

2. **P2 — prior review finding W10 remains instance-fixed.** The third review explicitly identified three stale W10 instructions, including “Everything machine-checkable gets a gate beside the four” ([record](/srv/worktrees/pr40-rereview3/reviews/2026-08-28-pr40-frontier-review-0472ef1.md:27)). This head fixed Scope and sequencing but left that exact present-tense instruction unchanged at [proposal line 197](/srv/worktrees/pr40-rereview3/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:197), immediately after declaring the work already completed. A pass organizer can still add a redundant fifth gate. The `PR40B-W10-PRE-MERGE-WORLD` disposition remains falsely `fixed`.

3. **P2 — exact-head evidence has recurred again.** The actual diff is **11 files and 830 insertions**, while [pr.md](/srv/worktrees/pr40-rereview3/pr.md:31) claims **7 and 673** and its Scope claims all seven lie outside `reviews/`. The same body correctly says eleven at line 158. The title counts 15 findings although the history table records 4+4+7+5 = **20**. This head added four review files and 157 insertions without updating those sibling claims. The claimed numeric enumeration therefore failed to prevent precisely the class it was introduced to prevent; `PR40B-BODY-NOT-EXACT-HEAD` is not fixed.

4. **P2 — the ledger still violates its location and recurrence contract.** All 18 locations exist, but the two rollback rows at [lines 382 and 389](/srv/worktrees/pr40-rereview3/pr.md:382) point to the `OsString` verdict, not a rollback defect; the exact-head row points to `CLAUDE.md:82`, which says only that four gates exist. The five findings from `0472ef1` also gained no new stable IDs or recurrence links, despite [MAINTAINING.md](/srv/worktrees/pr40-rereview3/MAINTAINING.md:64) requiring every actionable finding and recurrent prior ID to remain in the ledger. A token-overlap anchor check has again accepted lines that are not about the findings.

5. **P2 — the CI failure is described incorrectly.** [pr.md](/srv/worktrees/pr40-rereview3/pr.md:165) says exit 143 means the helper “was terminated on a bound, not failed.” The test itself explicitly sends SIGTERM at [proc.rs line 7960](/srv/worktrees/pr40-rereview3/src/agent/proc.rs:7960); `wait_for_exit` returns `None` on expiry, while this run returned status 143 and then failed the success assertion at line 7966. Thus no timeout-bound termination is evidenced: the helper died from the test’s SIGTERM and failed its expected-success contract. This does not attribute the failure to this documentation PR or require a retry, but the body’s account must be corrected. Its “ten green legs” is also unsupported: the listed successes total eight CI leaf jobs, or nine including PR policy, while the aggregate was red.

6. **P2 — one new review record lacks the required full SHA.** The documents-lens record gives only `bdd64f5` in its Reviewed SHA field ([record](/srv/worktrees/pr40-rereview3/reviews/2026-08-28-pr34-documents-lens-bdd64f5.md:6)); the full commit is `bdd64f5479e9a78aa03bb486d6ccec024a878c7d`. The other three records use full SHAs, and [MAINTAINING.md](/srv/worktrees/pr40-rereview3/MAINTAINING.md:33) expressly requires the full reviewed head SHA.

The four reproduced review bodies are otherwise byte-for-byte faithful, their history-table verdict/count pairs are correct, the commit table is correct, `pr.diff` exactly matches Git, and the non-Markdown count is 0. No panic, `anyhow`, path-portability, or `DESIGN.md:222` rule is implicated.

VERDICT: CHANGES_REQUIRED