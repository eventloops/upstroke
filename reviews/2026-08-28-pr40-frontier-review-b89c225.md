# PR #40 — frontier review record, `b89c225`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, seven findings** |
| **Reviewed SHA** | `b89c2257cae8f289a7c29d9f0094d89977113f9e` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| Inputs | the 715-line diff, the tree at that sha, `review-documents.md`, and the prior review of `f60df06` |

The second frontier review of #40, at a head CI had confirmed green. Seven findings, all verified and all real: six the branch's own and one inherited. The most consequential is that the previous round's W5 repair failed in the way its own commit message had named — it changed one count and left the next one false six lines below.

## The review, verbatim

## Findings

1. **P1 — W8 orders work the tree says is already complete.** The proposal says 93 of 265 catalogue entries “have never been applied” and instructs W8 to apply them ([proposal:161](/srv/worktrees/pr40-rereview/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:161)). Existing G2 evidence says PR7 applied the broader 115-entry cut and that W8 “therefore becomes re-measurement” ([evidence:78](/srv/worktrees/pr40-rereview/reviews/2026-08-25-pr7-g2-evidence.md:78), [evidence:87](/srv/worktrees/pr40-rereview/reviews/2026-08-25-pr7-g2-evidence.md:87)). Sequence: W8 follows the binding plan → repeats an obsolete 93-entry application → omits the additional 22 driver mutations and the required post-merge remeasurement → exit criterion 3 can be claimed against the wrong population.

2. **P2 — the W5 repair changed one “eleven” and left the next one false.** W5 now correctly opens with twelve readers ([proposal:124](/srv/worktrees/pr40-rereview/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:124)), then says “Collapse, rename, or regroup the eleven” six lines later ([proposal:130](/srv/worktrees/pr40-rereview/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:130)). Its “36 existing call sites” is stale too. The charter measured 36 at `3c09f6e`; since then the tree added six production reader calls and removed none—two `open_no_attempt`, two `frozen_rung_binding`, one `predicted_region`, and one `run_is_ending`—making 42. A W5 implementation scoped from this text can leave reader twelve and six consumers outside the redesign it claims is exhaustive. The ledger nevertheless marks this finding fixed.

3. **P2 — W10 still describes the pre-merge standards/gate state.** It instructs the pass to land a new `STANDARDS.md` or CONTRIBUTING section and add a bash gate “beside the seven” ([proposal:175](/srv/worktrees/pr40-rereview/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:175)). The tree already has the normative [CODING_STANDARDS.md](/srv/worktrees/pr40-rereview/CODING_STANDARDS.md:3), four cargo gates plus four bash gates, and `CLAUDE.md` correctly says four bash gates ([CLAUDE.md:82](/srv/worktrees/pr40-rereview/CLAUDE.md:82)). An implementer can create a second normative document or mistake the existing eighth gate for W10’s promised new enforcement. This is the same stale gate-count claim repaired in the body but not propagated to the proposal.

4. **P2 — the W4 deletion repair left the decision record’s sibling instruction stale.** The proposal now mandates deleting §1’s Known Conflicts block “in full” ([proposal:99](/srv/worktrees/pr40-rereview/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:99)). The decision record still calls it “a one-line master docs edit citing this record” ([record:75](/srv/worktrees/pr40-rereview/decisions/2026-08-25-commandspec-program-osstring.md:75)); the actual block is the heading and paragraph at [CODING_STANDARDS.md:36](/srv/worktrees/pr40-rereview/CODING_STANDARDS.md:36). One W4 implementer can retain a one-line resolution/citation under the heading while another deletes the block, each following one of the two documents.

5. **P1 — the Class A finding remains `deferred` in the exact-head ledger after being resolved.** The charter now unambiguously suspends Class A ([charter:107](/srv/worktrees/pr40-rereview/decisions/2026-08-24-pr3-layer-freeze-charter.md:107)), and the body calls the matter closed ([pr.md:230](/srv/worktrees/pr40-rereview/pr.md:230)). But the ledger still says “reported to the owner rather than resolved” with disposition `deferred` ([pr.md:268](/srv/worktrees/pr40-rereview/pr.md:268)). A reviewer or later slice reading the ledger sees an unresolved P1 where the governing document says the owner settled it.

6. **P2 — the rollback repair still contains an impossible claim.** The two principal rollback strategies are now sound, but [pr.md:252](/srv/worktrees/pr40-rereview/pr.md:252) says reverting `f60df06` alone leaves the verdict scheduled and the consequences done. `f60df06` is the commit that introduced the scheduled verdict; reversing it cannot preserve that verdict, and its reverse patch no longer applies cleanly at this head. The later `881df31` changed the consequences to future tense. Thus the stated result is not producible, while the ledger calls the rollback finding fixed.

7. **P2 — the body is not exact-head evidence in several recomputable places.**

   - It claims `594 insertions`; the exact diff is `7 files changed, 658 insertions(+)`.
   - Its four-commit list omits `881df31`; excluding the original payload and concurrent `fb1d874`, there are five branch repair/ruling commits.
   - It says four owner decisions while its own sections assert five, omitting Class A from the four-item enumeration.
   - It says the frontier findings are the ledger’s “last five rows”; they occupy seven rows.
   - Its claim that locations bind this head is false: the proposal-route row points to line 217 although the repaired criterion is at line 249, and the W5 row points to line 99 although W5 starts at line 123.

The operative moratorium precedence, scheduled `OsString` verdict, future-tense refusal consequence, durable-schema boundary, and “§1 incomplete” ruling otherwise match the tree. Scope is exactly seven Markdown paths; `src/topology/**`, `DESIGN.md:222`, and `CODING_STANDARDS.md` are untouched, so no panic, `anyhow`, or path-portability rule is implicated.

VERDICT: CHANGES_REQUIRED