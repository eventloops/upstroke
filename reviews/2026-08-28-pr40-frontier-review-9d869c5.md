# PR #40 — frontier review record, `9d869c5`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, seven findings** |
| **Reviewed SHA** | `9d869c52af7bc646c42f64590353b4f222e17abf` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the six committed prior review records, and the pull request's title and body |

The seventh recorded round, and the first in which the generated block itself was
checked and found accurate — *"the generated block itself is accurate, but its
surrounding prose repeats the recorded failure pattern"*. Two findings are the sharpest
this branch has had on its own ledger: the location rule introduced one head earlier
cannot satisfy `MAINTAINING.md`'s exact-reviewed-sha requirement, because a review record
is committed by a **later** head than the one its finding was measured at; and four
anchors had been silently pushed onto neighbouring clauses by a repair that moved the
proposal, passing a token-overlap check on words like "pass" and "reader".

It also catches a trend claim the body's own generated table refutes: the finding counts
are 4, 4, 7, 5, 6, 6, and the body said they were falling.

## The review, verbatim

Changes required. The generated block itself is accurate, but its surrounding prose repeats the recorded failure pattern.

1. The newly committed review is not fully answered. Its first finding says the five-ruling list omits W8 and W10, making seven ([record](/srv/worktrees/pr40-r7/reviews/2026-08-28-pr40-frontier-review-1aa8b89.md:24)). The title, Summary, and owner-decision section still say five ([pr.md](/srv/worktrees/pr40-r7/pr.md:1), [pr.md](/srv/worktrees/pr40-r7/pr.md:112)). No ledger row fixes or rejects that part; `PR40E-ANSWERING-HEAD-OMITTED-ITS-RECORD` preserves only the custody issue. Even if “seven” is disputed, silently dropping the actionable claim violates the ledger contract.

2. Scope still describes a smaller diff. It says every touched file is under `decisions/`, `proposals/`, or is `DESIGN.md` ([pr.md](/srv/worktrees/pr40-r7/pr.md:153)); the generated inventory contains six files under `reviews/`. Sequence: a reviewer follows formal Scope, omits the review records, and misses precisely the fidelity/SHA class this head added them to expose. The generated inventory makes the widening visible elsewhere, but the PR does not do its Scope claim exactly.

3. `9d869c5` made the CI chronology stale. The heading and narrative call `c3e5b20` “two heads back” ([pr.md](/srv/worktrees/pr40-r7/pr.md:221), [pr.md](/srv/worktrees/pr40-r7/pr.md:230)), and the ledger repeats it ([pr.md](/srv/worktrees/pr40-r7/pr.md:576)). On the first-parent chain, `c3e5b20` is `HEAD~3`: `c3e5b20 → 1aa8b89 → 472e813 → 9d869c5`. An operator following “two heads back” inspects `1aa8b89` and the wrong workflow runs. The generated current-head CI account itself matches the preserved API results.

4. At least four ledger locations were not re-anchored after `472e813` shifted the proposal:

   - `DOCS-PROPOSAL-RETIRES-BC-ROUTES` points to line 270, W8’s criterion; the Class A/B/C repair starts at line 277.
   - `PR40-W5-STALE-READER-COUNT` points to line 126, W4 prose; the repaired W5 count is at line 131.
   - `PR40B-W8-ORDERS-FINISHED-WORK` points to line 165, W7; W8 starts at line 170.
   - `PR40B-W5-SIBLING-COUNT-AGAIN` points to the unrelated 36-call-site measurement at line 134; the repaired “all twelve” instruction is line 139.

   The token checker passed these using generic overlaps such as “pass”, “reads”, “them”, and “reader”. Following the first anchor, for example, makes an auditor inspect an entirely different exit criterion.

5. The new body-anchor rule does not provide the “exact reviewed SHA and file/line” required by [MAINTAINING.md](/srv/worktrees/pr40-r7/MAINTAINING.md:64). A review-record line is where the defect was reported, not where the mutable body defect existed; pairing it with `9d869c5` also replaces the actual reviewed SHA with the current unreviewed head. The rule is not applied consistently either: body defects still point to `CLAUDE.md:82`, the charter, and `src/agent/proc.rs:7960` ([pr.md](/srv/worktrees/pr40-r7/pr.md:549), [pr.md](/srv/worktrees/pr40-r7/pr.md:556), [pr.md](/srv/worktrees/pr40-r7/pr.md:562)). Of the checker’s two flags, `PR40-OUT-OF-SCOPE-RECONCILIATIONS` is a false positive—`CODING_STANDARDS.md:38` is relevant conflict evidence—but `PR40B-LEDGER-STALE-AFTER-RULING` violates the body’s own rule and should locate the review finding at `b89c225` line 27.

6. Volatile facts still escape the generated block. The CI section manually repeats the generated non-Markdown count as `0` ([pr.md](/srv/worktrees/pr40-r7/pr.md:249)), despite its ledger disposition claiming no count remains outside the block. The heading separately repeats the live CI conclusion as “green”. Round ordinals also remain at lines 280–292, 421, and 572. Worse, “the count of findings is falling” ([pr.md](/srv/worktrees/pr40-r7/pr.md:286)) is unsupported by the generated sequence `4, 4, 7, 5, 6, 6`; the latest three rise and then remain flat. A later body edit or scope change can therefore update the generated facts while leaving these siblings contradictory.

7. The hard merge-order claim is not reproducible as written. The three decision files are absent from the local `standards/pr7-worklist` ref I could inspect, so the premise appears presently true, but [pr.md](/srv/worktrees/pr40-r7/pr.md:466) binds it to no #41 head SHA, ref, or command and also asserts facts about all three mutable PR bodies. Sequence: #41 is updated or rebased, its tree gains the records, and #40 continues asserting a hard dependency whose premise no longer holds. An exact-head claim about another PR needs an exact external head or explicit as-of qualification.

I confirmed `pr.diff` matches Git, the 13-file/913-insertion inventory, 11/23 commit counts, six-round/32-finding table, CI rows and leaf jobs, and the new review record’s byte-faithful body, metadata, and full SHA. All changed files are Markdown, so the panic, `anyhow`, `std::path`, and frozen source-path rules are not implicated.

VERDICT: CHANGES_REQUIRED