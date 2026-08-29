# PR #42 — frontier review record, `31e24cc`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `31e24cc9fc2749eaac1a20745b304010da7e1dcc` |
| Pull request | eventloops/upstroke#42, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 42`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name — `CI` run `33157989853`, `Pull request policy` run `33157989834`, both first-attempt successes |
| Inputs | the diff, the tree at that sha, and the pull request's title and body |

The first review of #42. Its third finding is the one that mattered: the seven routed
rows were said to cite `CODING_STANDARDS.md` §8, and five of them cite §14. The owner
corrected the grounds on the same day and kept the routing, because §14 is MUST-tagged
too — so the verdict the cluster receives is unchanged and the evidence for it is not.

The record is committed here rather than left in a home directory on one build box,
which is the custody rule the sibling pull request #40 conceded in its own fifth round:
a head cannot contain the review that judges it, so the review of a head is committed by
whichever head answers it.

## The review, verbatim

## Findings

1. The seven “contract rows” do not satisfy the ledger contract. [MAINTAINING.md](/srv/worktrees/pr42-r1/MAINTAINING.md:64) requires severity, full SHA plus `file:line`, concrete `A -> B -> failure` sequence, provenance, category, prior ID, prevention, and canonical disposition. [The added rows](/srv/worktrees/pr42-r1/reviews/FINDINGS.md:163) contain none of those except an abbreviated SHA and owner. The PR ledger remains `None yet`; the validator consequently passes while skipping all seven findings. They also violate [CODING_STANDARDS.md:601](/srv/worktrees/pr42-r1/CODING_STANDARDS.md:601): every standards finding must name the applicable enforcement-map row and mechanism or `review-only`.

2. The claimed provenance, merge ordering, and rollback are false at this exact head. [The work-list](/srv/worktrees/pr42-r1/reviews/2026-08-25-pr7-standards-worklist.md:29) contains none of these seven; `reviews/2026-08-28-pr7-standards-triage.md` and `decisions/2026-08-25-checkpoint-merges.md` do not exist at HEAD. Yet [pr.md](/srv/worktrees/pr42-r1/pr.md:53) says the observations are already filed, recommends merging #42 before #41, says nothing dangles, and claims a revert leaves the observations in the work-list. Concrete failure sequence: merge #42 first as recommended → authoritative rows assert provenance to records absent from the repository → revert #42 → the only repository copies disappear, contrary to the rollback claim.

3. This is not an exact move: five observations have been silently reclassified. At referenced #41 head `ea25033`, the source review cites §14 for `PRIVATE-ROOT-LEXICAL-COMPARE`, `OWNER-RECORD-LEXICAL-AUTH`, both component-write findings, and `OWNERSHIP-PROOF-UNCANONICAL`; only `PRIVATE-ROOT-NO-CONTAINMENT` and `CONTAINER-LEXICAL-CONFINEMENT` cite §8. The new rows and [PR body](/srv/worktrees/pr42-r1/pr.md:16) assert that all seven cite §8. Section 14 is also MUST-tagged, so this does not necessarily reverse their routing, but the stated “same §8 keyword is the whole reason” evidence is false and the diff substantively changes five findings beyond routing them.

4. The hash claim is too strong and does not provide exact citations. All seven hashes resolve, but the first two cover only helper definitions: `31b6…` is [recover.rs:468](/srv/worktrees/pr42-r1/src/engine/topology/recover.rs:468), excluding both the explicit comparison at line 340 and its rationale at line 460; `388c…` is [recover.rs:753](/srv/worktrees/pr42-r1/src/engine/topology/recover.rs:753), excluding the owner-record authentication at line 632. The third “documented” hash likewise excludes its documentation. Thus changing the rationale while leaving the hashed code intact preserves the digest while falsifying the rows’ “this site is documented” claim. Moreover, #40’s proposed W10.4 requires the entire reviewed file to remain byte-identical and re-derivation otherwise; it does not authorize region-only salvage.

What did check out: `pr.diff` exactly matches `3e5212d…31e24cc`; only `reviews/FINDINGS.md` changes; exactly seven rows are appended within §2 before §3; every digest maps to real, broadly relevant source; and no frozen source, `DESIGN.md`, `CODING_STANDARDS.md`, or `decisions/` file is edited.

VERDICT: CHANGES_REQUIRED