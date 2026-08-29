# PR #42 — frontier review record, `f16c472`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, seven findings** |
| **Reviewed SHA** | `f16c4724eee7f779db87b6cfd77a8c6a91029379` |
| Pull request | eventloops/upstroke#42, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 42`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, both committed prior review records, and the pull request's title and body |

The third review of #42, and the count rose rather than fell. Its first finding is this
branch's own recurrence, for the third time on the same row: a region was widened where the
previous reviewer pointed and asserted sufficient everywhere else, and the one row nobody
had pointed at still excluded the rationale — which for the ownership proof lives on the
**write** side, in a different file. Its third finding is sharper than a bookkeeping miss:
a failure sequence supplied in the previous repair does not follow from the code, because
the two `normalize` calls are independent and only the failing one falls back.

Findings six and seven are prose restating facts the diff had moved — the file-set claim,
the row count in three sections — which is the class this branch is now deleting rather
than correcting.

## The review, verbatim

## Findings

1. **P1 — one repaired digest still excludes its rationale.** All ten recorded digest values reproduce, but `PR7-STD-OWNERSHIP-PROOF-UNCANONICAL` hashes only `rundir.rs:1563-1595` ([row](/srv/worktrees/pr42-r3/reviews/FINDINGS.md:168)). The proof’s documented contract is at [rundir.rs:1451](/srv/worktrees/pr42-r3/src/rundir.rs:1451), while the explicit rationale for the fallback is at [create.rs:1983](/srv/worktrees/pr42-r3/src/engine/topology/create.rs:1983): the same lexical fallback is deliberately used on both sides. Edit either rationale while leaving `1563-1595` unchanged and `c3dcc15d…` still verifies. Thus the body’s claim that every region spans rationale and decision is false; this is the same repair-only-where-pointed recurrence.

2. **P2 — the W10.4 withdrawal is incomplete.** The standing work-list still says entries are “salvaged by-hash … per … W10.4” ([work-list](/srv/worktrees/pr42-r3/reviews/2026-08-25-pr7-standards-worklist.md:21)). That is active policy text, unlike the historical quotations in the committed reviews. Concrete failure: add the later work-list entries, change another part of a reviewed file while preserving one region, then follow this section and salvage the row; W10.4 actually requires whole-file byte identity and re-derivation otherwise. The seven moved rows were corrected, but the repository still makes the rejected claim.

3. **P2 — a newly supplied canonical failure sequence does not follow the code.** `PR7-STD-PRIVATE-ROOT-LEXICAL-COMPARE` says failure to canonicalize “either root” makes `normalize` return lexical paths “for both sides” ([row](/srv/worktrees/pr42-r3/reviews/FINDINGS.md:163)). The calls are independent at [recover.rs:340](/srv/worktrees/pr42-r3/src/engine/topology/recover.rs:340), and [normalize](/srv/worktrees/pr42-r3/src/engine/topology/recover.rs:468) falls back only for the call that failed. One failure therefore does not imply two lexical results. Even two equal lexical `PathBuf`s do not establish two different directories without an additional retarget/race step that the sequence omits. The underlying owner question may remain open, but this claimed concrete sequence is not evidence for it.

4. **P2 — the ledger’s body-only-location disclosure omits a third such row.** The two named contract anchors are honestly described, but `PR42-PROVENANCE-AND-ROLLBACK-FALSE` is also a finding against PR-body text. It anchors at `31e24cc/reviews/2026-08-25-pr7-standards-worklist.md:33`, whose content is the correct `_(none yet)_` placeholder—not the defect. That is evidence disproving the body, exactly like the disclosed contract anchors are evidence of obligations. This contradicts the ledger’s own statement that a location says where a defect was and makes the “two rows” disclosure incomplete.

5. **P2 — established first-bad commits are still omitted.** `PR42-W10-4-CITED-FOR-SOMETHING-IT-DOES-NOT-SAY` uses `—` even though `31e24cc` introduced the exact W10.4 claim ([ledger](/srv/worktrees/pr42-r3/pr.md:217)). The two `fix_regression` rows likewise provide prior IDs but omit `f5204bb`, the commit containing the defective repair. [MAINTAINING.md](/srv/worktrees/pr42-r3/MAINTAINING.md:71) requires both the first-bad commit where history establishes it and the prior ID when a defect recurs.

6. **P2 — the cross-PR repair still leaves an unsupported premise outside either evidence bucket.** “The three file sets are disjoint” ([pr.md](/srv/worktrees/pr42-r3/pr.md:182)) is neither something this tree demonstrates nor labelled as the owner’s statement, and no #40/#41 heads are pinned. That is one of the exact mutable-branch premises the preceding paragraphs say this tree cannot establish, and it contradicts the ledger disposition’s claim that every premise is marked as tree-verified or owner-reported.

7. **P2 — the eighth row widened the stated scope without updating Scope, Validation, or Rollback.** The diff adds eight rows, at `FINDINGS.md:163-170`, while Scope says seven ([pr.md](/srv/worktrees/pr42-r3/pr.md:60)), placement validation says seven, and rollback says it removes seven ([pr.md](/srv/worktrees/pr42-r3/pr.md:145)). The generated revert command itself is sound: it restores the base tree and removes all eight rows plus both review records. The claims describing it are not.

The new §9 row otherwise checks out: §9’s MUST governs timeout/cancellation/cleanup and stdout/stderr-size behavior; its region includes the stream-separation-only comment and blocking `Command::output`; its failure sequence and `review-only` routing follow. The authority correction matches `decisions/README.md`. The exact diff touches only the three advertised review Markdown files, so no source, frozen-design, panic/anyhow, path-portability, or immutable-decision rule is violated.

VERDICT: CHANGES_REQUIRED