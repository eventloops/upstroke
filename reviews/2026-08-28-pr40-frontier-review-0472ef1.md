# PR #40 — frontier review record, `0472ef1`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `0472ef1aaca01f1feeca7f1df529d032eab3037f` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN — `CI` and `Pull request policy` both success on the newest run per name |
| Inputs | the 673-insertion diff, the tree at that sha, and both prior review records |

The third review of #40. Three of its five findings — W8, W10 and the charter's
insertion count — are one class: a repair that fixed the sentence a reviewer quoted
and left a sibling claim stale elsewhere. That is the fourth recurrence of that
class on this branch, and it is why the answering commit builds a mechanical
enumeration of every co-referring numeric claim rather than fixing five more
sentences.

## The review, verbatim

There are five blocking documentation defects.

1. **P1 — W8 still orders the work that its repair forbids.** W8 now says “re-measurement, not application” and warns that reapplication repeats completed work ([proposal:163](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:163)). The sequencing section still calls W8 “catalogue application” ([proposal:238](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:238)). An implementer following that summary can reapply the obsolete cut and claim exit criterion 3 against the wrong population—the exact prior P1. The ledger’s claim that the plan and evidence can no longer order different work is unsupported.

2. **P2 — W10 still has future-work instructions for completed work.** Its main item says the standards document and gates already landed and must not be duplicated ([proposal:186](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:186)). But Scope still includes master for “W10’s standards document and its mechanical gates” ([proposal:31](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:31)); the item still says machine-checkable rules get a gate “beside the four” ([proposal:195](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:195)); and sequencing still says W10’s standards PR “targets master and merges first” ([proposal:229](/srv/worktrees/pr40-rereview2/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:229)). A pass organizer can still open or wait for a redundant standards/gate PR. Finding `PR40B-W10-PRE-MERGE-WORLD` is only instance-fixed.

3. **P2 — the rollback repair is stale at this exact head.** The stated history ends at `b89c225`, omitting `0472ef1` ([pr.md:248](/srv/worktrees/pr40-rereview2/pr.md:248)), and the “undo repairs” command likewise omits it ([pr.md:253](/srv/worktrees/pr40-rereview2/pr.md:253)). Therefore it cannot return the documents to `5763fe3`: `0472ef1`’s W8/W10 changes remain, and reversing `881df31` either conflicts with the later W5 edit or restores the opening “eleven” while leaving the later “twelve.” The ledger also contradicts itself: the earlier rollback row still presents the impossible single-revert result as prevention ([pr.md:286](/srv/worktrees/pr40-rereview2/pr.md:286)), while the later row says that result was impossible ([pr.md:293](/srv/worktrees/pr40-rereview2/pr.md:293)).

4. **P2 — the body is still not exact-head evidence.** It says “four commits of its own” immediately before enumerating six ([pr.md:12](/srv/worktrees/pr40-rereview2/pr.md:12)). It also says the review of `b89c225` was followed by two head moves including `fb1d874` ([pr.md:221](/srv/worktrees/pr40-rereview2/pr.md:221)); ancestry shows `fb1d874` predates `b89c225`, and only `0472ef1` follows that reviewed head. Thus `PR40B-BODY-NOT-EXACT-HEAD` cannot be marked fixed.

   All eighteen numeric ledger locations exist, but two rollback rows cite [`decisions/README.md:1`](/srv/worktrees/pr40-rereview2/decisions/README.md:1), merely `# decisions/`, rather than the claimed body defect. That violates the exact file/line requirement and explains why the syntactic anchor check passed while rollback went stale again.

5. **P2 — the charter retains another stale eleven-reader count.** The Class A bullet says “PR7’s eleven insertions” ([charter:103](/srv/worktrees/pr40-rereview2/decisions/2026-08-24-pr3-layer-freeze-charter.md:103)), while the same record now identifies PR7’s twelfth insertion, `open_no_attempt` ([charter:139](/srv/worktrees/pr40-rereview2/decisions/2026-08-24-pr3-layer-freeze-charter.md:139)). The former needs an explicit historical qualifier such as “first eleven” or a SHA; otherwise an isolated Class A audit again scopes PR7 one reader short.

The decision to avoid asserting 42 in W5 is sound: anchoring 36 to `3c09f6e` and requiring exact-head remeasurement keeps W5 scoped without inventing a replacement count. The five owner rulings otherwise match the tree. `pr.diff` exactly matches the seven-path, 673-insertion documentation diff; no source, panic, `anyhow`, or path-portability rule is implicated.

VERDICT: CHANGES_REQUIRED