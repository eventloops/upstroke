# PR #42 — frontier review record, `4f34c62`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `4f34c62a5ee1df472a9008fe629d0a91574b024a` |
| Pull request | eventloops/upstroke#42, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 42`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed prior review records, and the pull request's title and body |

The fifth review of #42, and the one that named the seat's own failure mode rather than a
defect in the documents. Four of its five findings are the same shape: **a ledger
disposition claiming a repair that was not made** — the merge-order judgement said to have
returned, the first-bad cells said to be filled, the anchors said to be marked in place, and
a blanket claim said to be replaced. Each was written into a guard while the edit that would
have made it true never reached the file, in two cases because an editing script raised
before writing.

## The review, verbatim

## Findings

1. **P2 — the promised structural deletion is still largely unapplied.** [pr.md:34](/srv/worktrees/pr42-r5/pr.md:34) and [pr.md:42](/srv/worktrees/pr42-r5/pr.md:42) duplicate the claim that recomputable facts were deleted, while prose still states quantities, chronology, and mutable outcomes: “more than once/one/most/another” at lines 16–22, tree and file state at lines 51–56, gate and placement claims at lines 60–68, “every/all” region claims at lines 70–82, review state at lines 86–95, and rollback state at lines 99–116. The opening claim that findings “leave” the work-list is false now: the diff never touches that file, whose row remains `_(none yet)_`. The new `PR42-STRIP-APPLIED-TO-DIGITS-ONLY` row claims a checker enforces the deletion, but no such checker is added or named, and the current body disproves it. A later row or file change therefore leaves these assertions stale—the exact failure this round claims to prevent.

2. **P2 — deletion still removed the owner’s required merge-order judgment.** The body has no merge-order section, yet [three dispositions](/srv/worktrees/pr42-r5/pr.md:132) claim it contains an owner ordering, its grounds, and a per-row section table; [PR42-DELETION-REMOVED-JUDGEMENT](/srv/worktrees/pr42-r5/pr.md:147) now claims those returned. None did. The owner’s ordering and reasons are judgment, not reconstructible from this diff or tree. Sequence: maintainer follows a `fixed` disposition → searches the body for the promised ruling → finds no ordering or grounds → must guess the dependency the body claims to supply.

3. **P2 — the first-bad/provenance repair was not applied and its new row repeats the defect.** [PR42-W10-4-CITED-FOR-SOMETHING-IT-DOES-NOT-SAY](/srv/worktrees/pr42-r5/pr.md:138) still has `—` instead of `31e24cc`; the canonical-fields and five-region regressions still omit `f5204bb`; the cross-file-region and sequence regressions still omit `aa31e525`; the work-list row omits `c0f9b36`; and the row-count regression omits `f16c472`. The two provenance cells previously identified as repair regressions remain `introduced_by_feature` at lines 143 and 145. Worse, [PR42-FIRST-BAD-CELLS-UNCHANGED](/srv/worktrees/pr42-r5/pr.md:148) itself supplies only the prior ID and omits the commit that introduced this recurrence. The mechanical validator accepts the nonempty cells, while a maintainer still cannot bisect to the defective repair.

4. **P2 — evidence and obligation anchors remain unmarked in place.** [pr.md:125](/srv/worktrees/pr42-r5/pr.md:125) and [PR42-EVIDENCE-ANCHORS-NOT-MARKED-IN-PLACE](/srv/worktrees/pr42-r5/pr.md:149) say every such row labels its anchor. The provenance/rollback row’s guard still does not say its work-list placeholder is evidence, the `MAINTAINING.md` anchors do not say they are obligations, and the new evidence-anchor row does not label its own `FINDINGS.md:163` location as evidence. Sequence: maintainer opens the promised defect location → finds correct content → concludes the finding is unsupported.

5. **P2 — the sequence-repair disposition claims an edit that was not made.** The two standing-ledger sequences now include the necessary retarget step, but [PR42-SEQUENCES-OMIT-THE-RETARGET-STEP](/srv/worktrees/pr42-r5/pr.md:150) also says the blanket “every sequence was walked” claim was replaced. That exact claim remains at [pr.md:109](/srv/worktrees/pr42-r5/pr.md:109), and is itself mutable state forbidden by this round’s structural rule.

I confirmed that `pr.diff` exactly matches the base-to-head Git diff, every ledger SHA/path/line resolves at its bound commit, and every recorded region digest reproduces. Only review Markdown changes, so the panic, `anyhow`, `std::path`, DESIGN-authority, and immutable-decision-file rules are not otherwise implicated.

VERDICT: CHANGES_REQUIRED