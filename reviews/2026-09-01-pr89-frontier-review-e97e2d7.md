# PR #89 — review effort re-scoped: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, three findings, recorded as written. All three repaired in the one documentation commit above the reviewed head, disclosed below; none meets the serious-P1 bar this pull request defines |
| **Reviewed SHA** | `e97e2d707997f8c2fce819fc01d2134e98e3abc1` |
| Pull request | eventloops/upstroke#89, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 89`, 90-minute per-pass limit |
| Inputs | the 476-line exact-head diff (11 files) assembled from the PR's own base `66523d3`, and the body |
| CI at the reviewed SHA | 11/11 success, uncancelled: `upstroke-ci` run 33545707872 (10 jobs) and `upstroke-pr-policy` run 33545707856 |
| Pass history | pass 1 `9c7cfa8`, five findings; pass 2 `18fa1a1`, five; pass 3 `4e1b7ad`, four; pass 4 `e97e2d7`, three (this record). All seventeen repaired; the pull request's ledger carries every row |

Merge on this verdict, disclosed. The owner directed in writing on 2026-09-01
that this pull request merge on its pass-4 verdict with findings below the
serious-P1 bar repaired or logged rather than iterated ("not absolutely
requiring everything to be fixed so we don't slow to a crawl while
developing"), applying the rule it introduces to itself. No fifth pass ran.
The three findings were repaired in one documentation-only commit above the
reviewed head; the owner's agent, acting under the owner's written merge
delegation, read that delta against the findings and the body records the
verification. The classification of all three as non-serious is the agent's,
made under that direction: in each case the controlling text in
`MAINTAINING.md` already stated the rule, and the failure needed an operator
to follow a summary or a notice against it.

## The verdict, verbatim

Blockquoted byte-for-byte from the reviewer's saved output
(`~/review-pr89-e97e2d7.md` on the review box, sha256 `dc28167239124fb0…`).

> 1. **P1 — the single-reviewer/serious-P1 limits are still missing from the central invalidation notice.** The notice authorizes “a stale pass plus an owner-verified repair delta containing no serious P1” without requiring that the original pass found no serious P1 or was single-reviewer ([invalidation notice](https://github.com/eventloops/upstroke/blob/e97e2d707997f8c2fce819fc01d2134e98e3abc1/decisions/2026-08-20-review-invalidation-scope.md#L159-L166)). `CLAUDE.md:142-150` and `CONTRIBUTING.md:10-13` repeat the unqualified lane, contradicting `MAINTAINING.md:43-45`.
>
>    Concrete failures:
>
>    - Panel reviews A → non-serious repair produces B → owner follows the notice and verifies B → B merges without rerunning every seat.
>    - A contains a serious P1 → B fixes it, so the *delta* contains a repair rather than a P1 → the same wording permits owner verification instead of the mandatory fresh pass.
>
>    This silently widens the lane beyond the PR’s claimed single-reviewer scope and makes the ledger’s “panel bypass fixed” assertion false.
>
> 2. **P2 — the living slice rule still requires review of every head.** `MAINTAINING.md:66-72` says slice PRs receive “a single-reviewer frontier review of each head,” while step 6 and the appended slice notice permit an unreviewed repair head. Sequence: A is reviewed → a non-serious finding is repaired in B → step 6 permits owner verification, but line 68 requires B to receive a frontier review. Both rules cannot be followed. “Triaged per step 6” does not make an unreviewed head a reviewed one.
>
> 3. **P1 — the template still makes evidence-backed and `MUST` findings waivable as `rejected`.** The checkbox permits either `fixed` or `rejected` merely “with the row saying why” ([template line 36](https://github.com/eventloops/upstroke/blob/e97e2d707997f8c2fce819fc01d2134e98e3abc1/.github/pull_request_template.md#L36)). That contradicts `MAINTAINING.md:144-150`, which says these findings block until repaired, and the preserved §3 rule that a failing test, reproduction, or mutation witness “blocks — not debatable” ([§3](https://github.com/eventloops/upstroke/blob/e97e2d707997f8c2fce819fc01d2134e98e3abc1/decisions/2026-08-20-automated-review-gate.md#L87-L97)). A valid Windows reproduction can be labeled `rejected` as “out of scope,” satisfying the checkbox while merging the reproduced defect. Rejecting a purported witness is defensible only after establishing it is not valid evidence; the template does not impose that condition.
>
> I also verified that `pr.diff` exactly matches the SHA’s eleven-path range, the claimed documentation tests pass, existing decision text is only appended to, and no Rust/`anyhow`/path surface changed.
>
> VERDICT: CHANGES_REQUIRED
## Disposition of the three findings

1. The lane's qualifiers. The 2026-08-20 invalidation notice, CLAUDE.md and
   CONTRIBUTING.md now say the owner-verified lane follows a single-reviewer
   pass that found no serious P1, and the notice adds that a serious P1 repair
   still returns for a fresh pass and a panel-reviewed candidate re-runs every
   seat; step 6's lane sentence carries the same qualifier. Ledger row
   `PR89-LANE-UNQUALIFIED-IN-NOTICE-AND-SUMMARIES`, fixed.
2. The slice rule. The paragraph after step 8 no longer promises a review of
   each head; slice pull requests receive a single-reviewer review triaged per
   step 6, and a repair head follows step 6's lane so every merged head is
   accounted for as step 7 requires. Ledger row `PR89-SLICE-RULE-EACH-HEAD`,
   fixed.
3. Rejection of evidence-backed findings. The template checkbox and the triage
   subsection now allow `rejected` for a `MUST` deviation or an evidence-backed
   finding only by a row showing the evidence is not valid (a `MUST` the code
   does not breach, a witness that does not reproduce on the head). Ledger row
   `PR89-TEMPLATE-REJECTED-WITHOUT-INVALIDITY`, fixed.
