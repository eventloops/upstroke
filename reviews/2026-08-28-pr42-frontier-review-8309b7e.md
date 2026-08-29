# PR #42 — frontier review record, `8309b7e`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `8309b7e0e01e06c2c12aea30cac8c764d8b128a5` |
| Pull request | eventloops/upstroke#42, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 42`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed prior review records, and the pull request's title and body |

The fourth review of #42. Its second finding is the one worth keeping: **deletion can lose
judgement**. Removing the merge-order section took the owner's ordering and its reasons out
of the tree, and left three ledger dispositions citing a section that no longer exists. An
ordering and its grounds are judgement, not a recomputable fact, and the rule that removes
counts should not have reached them.

Its fifth finding is the sibling-staleness class inside a failure sequence: two rows moved
from a canonicalization failure straight to "a different directory that spells the same",
which does not follow without a retarget step neither row stated.

## The review, verbatim

## Findings

1. **P2 — the promised structural deletion is incomplete.** There is no generated block, yet prose still states quantities and mutable state: “corrected twice,” “first/most/second” (`pr.md:7,17-20`), only-copy and cross-branch state (`:47-49`), local gate conclusions (`:53-56`), “three consecutive rounds” (`:67-68`), review state (`:77-85`), zero runtime risk (`:90`), and rollback/tree state (`:103-107`). This directly falsifies `pr.md:35-40` and violates this round’s explicit rule. The opening “leave the cleanliness work-list” is also not an exact description of the diff: the work-list is unchanged and `pr.md:47-49` admits it never contained these rows here.

2. **P2 — deletion removed non-recomputable judgement and left dispositions citing artifacts that no longer exist.** `PR42-PROVENANCE-AND-ROLLBACK-FALSE` says a merge-order section carries the corrected hard ordering; `PR42-MERGE-ORDER-ASSERTS-WHAT-THIS-TREE-CANNOT-SHOW` says that order and its grounds are present; and `PR42-SECTION-CITATION-WRONG-FOR-FIVE` says the body contains a per-row section table. The body has neither section nor table, while `PR42-DISJOINT-CLAIM-UNSUPPORTED` explicitly says the section was deleted. The owner’s ordering and reasons are judgement, not recomputable facts, and no complete copy remains in this tree. Either they were needed—then deletion lost required maintainer guidance—or they were not, in which case the fixed dispositions must stop claiming they exist.

3. **P2 — the previous first-bad finding is recorded as fixed without changing the cited cells.** The prior review established that `PR42-W10-4-CITED-FOR-SOMETHING-IT-DOES-NOT-SAY` needed `31e24cc`, while the two `fix_regression` rows needed both `f5204bb` and their prior IDs. At `pr.md:127-129`, those rows still contain only the prior ID or an em dash. Nevertheless, `PR42-FIRST-BAD-OMITTED` claims every establishable commit is now present. The repair also repeats the defect in new rows: the cross-file-region and sequence regressions omit their introducing `aa31e525`, the work-list row omits `c0f9b36`, and the stale-row-count regression omits `f16c472`. Additionally, `PR42-THIRD-BODY-ONLY-ROW-UNMARKED` and `PR42-DISJOINT-CLAIM-UNSUPPORTED` describe defects introduced by repairs but label their provenance `introduced_by_feature`, not `fix_regression`. A maintainer following these “fixed” rows still cannot bisect to the defective repairs.

4. **P2 — the prior body-location finding is also not fixed.** `pr.md:114-117` says every evidence/obligation anchor is marked in its own guard. The `PR42-PROVENANCE-AND-ROLLBACK-FALSE` guard does not say that its location is merely evidence; opening its cited `31e24cc` location shows a correct `_(none yet)_` placeholder, not the defect. The same omission exists in newly added body findings anchored at `MAINTAINING.md` obligations or `FINDINGS.md` evidence. Sequence: maintainer follows the promised defect location → finds correct content → concludes the finding is unsupported, because the claimed in-place qualification is absent.

5. **P2 — two sibling failure sequences still do not follow without an omitted retarget step.** `PR7-STD-OWNER-RECORD-LEXICAL-AUTH` and `PR7-STD-OWNERSHIP-PROOF-UNCANONICAL` jump from canonicalization failure and equal lexical strings to “a different directory that spells the same.” Equal spellings do not establish different directory identities. A valid harmful sequence needs an additional event: write the record while spelling `L` denotes directory A → replace/retarget `L` to directory B while preserving the other matching fields → make canonicalization unavailable → lexical equality authenticates B using A’s record. Neither row states that transition. This is the same sibling-staleness pattern the prior review identified, despite `pr.md:100-101` claiming every sequence was walked.

What checked out: `pr.diff` is byte-identical to the exact `3e5212d…8309b7e` diff; only review Markdown changes, so no Rust panic/`anyhow`/path rule or immutable-decision rule is implicated. Every recorded region digest reproduces with the documented newline scheme, the widened cross-file region covers both rationales, all ledger locations resolve at their own SHA, the clause/enforcement-map mappings are correct, and the rewritten private-root sequence now respects the two independent `normalize` calls.

VERDICT: CHANGES_REQUIRED