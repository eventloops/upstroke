# PR #40 — frontier review record, `f60df06`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `f60df069ecfebb9ba7db65d956062c76f14de9ce` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| Inputs | the 651-line diff assembled from the PR's own base, the tree at that sha, `review-documents.md`, and the two commits answering it |

The first frontier review of #40. All four findings were verified against the tree before anything was changed; all four were real, and three were defects the branch had introduced — two of them by the Option-2 patch.

## The review, verbatim

## Findings

1. **P1 — the moratorium still has two conflicting operative routes.** The charter forbids any post-record reader before the pass ([charter:26](/srv/worktrees/pr40-review/decisions/2026-08-24-pr3-layer-freeze-charter.md:26)), but also permits Class A readers without pre-approval ([charter:98](/srv/worktrees/pr40-review/decisions/2026-08-24-pr3-layer-freeze-charter.md:98)) and says those classes govern outside passes ([charter:124](/srv/worktrees/pr40-review/decisions/2026-08-24-pr3-layer-freeze-charter.md:124)). Concrete failure: before the pass, a slice adds a thirteenth delegating reader plus the required ledger row; it complies with Class A while violating the moratorium. No precedence rule resolves that conflict.

   The sibling audit also missed the pass plan: W5 still says to survey and consolidate eleven readers ([proposal:92](/srv/worktrees/pr40-review/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:92)), despite the approved twelfth being recorded ([FINDINGS.md:570](/srv/worktrees/pr40-review/reviews/FINDINGS.md:570)) and present in code ([fold.rs:1012](/srv/worktrees/pr40-review/src/topology/fold.rs:1012)). Thus finding 1 is not fully discharged.

2. **P1 — the scheduled `OsString` ruling remains split-brained.** The repaired record says the direction is settled and the finding stays open until W4 ([record:3](/srv/worktrees/pr40-review/decisions/2026-08-25-commandspec-program-osstring.md:3)). But the supposedly accurate coding standard still calls it an unresolved owner question and says `String` governs “until that ruling” ([CODING_STANDARDS.md:38](/srv/worktrees/pr40-review/CODING_STANDARDS.md:38)); the ruling has now happened. The pass proposal likewise still says the owner will decide either outcome ([proposal:82](/srv/worktrees/pr40-review/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:82)), while the ledger says the matter is “not an open question” and “settled” ([FINDINGS.md:131](/srv/worktrees/pr40-review/reviews/FINDINGS.md:131)).

   Concrete failure: at W4, one implementer follows the new record and widens; another follows the coding standard/proposal and blocks awaiting a ruling or preserves the refusal. The original divergent-authority harm remains. The record also says the refusal test “is replaced” and its cause deleted ([record:65](/srv/worktrees/pr40-review/decisions/2026-08-25-commandspec-program-osstring.md:65)), but that test still exists ([bin.rs:457](/srv/worktrees/pr40-review/src/agent/bin.rs:457)). The edit additionally leaves the incomplete sentence “This addresses” ([record:19](/srv/worktrees/pr40-review/decisions/2026-08-25-commandspec-program-osstring.md:19)). None of this requires editing frozen `DESIGN.md:222`; the surrounding documents need reconciliation.

3. **P2 — the exact-head validation narrative is false.** The body says `CLAUDE.md` still lists three bash gates ([pr.md:146](/srv/worktrees/pr40-review/pr.md:146)); at this head it explicitly lists four ([CLAUDE.md:82](/srv/worktrees/pr40-review/CLAUDE.md:82)). C3 of the passing docs gate enforces that equality ([test-docs-consistency.sh:18](/srv/worktrees/pr40-review/.github/scripts/test-docs-consistency.sh:18)), so the claimed passing gate directly contradicts the body’s alleged discrepancy.

4. **P2 — the rollback instruction cannot produce its claimed state.** The body says one `git revert` returns the branch to `5763fe3` ([pr.md:188](/srv/worktrees/pr40-review/pr.md:188)). The history is `5763fe3 → 877278d → 55d50ae (merge) → f60df06`. Reverting `f60df06` leaves the repaired documents; reverting the eventual PR merge returns to base `3e5212d`, where the documents do not exist. Following this instruction can therefore leave a partial contradictory rollback or remove the documents entirely.

The checkpoint labels and measured G2 count recompute correctly, the B/C-route repair matches the charter, and the public-filing policy expressly permits the owner’s chosen disposition. The net diff is exactly seven Markdown paths; `src/topology/**` and `DESIGN.md:222` are untouched, so no panic/`anyhow`/path or frozen-layer scope rule is violated.

VERDICT: CHANGES_REQUIRED