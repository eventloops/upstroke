# PR #42 — frontier review record, `f59a218`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `f59a2185632d747435ad6586845dbe0783b55108` |
| Pull request | eventloops/upstroke#42, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 42`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed record of the first review, and the pull request's title and body |

The second review of #42. Its first finding is the one that generalises: the previous
head bound every ledger row to itself, an unreviewed head, when a row's whole job is to
say where a defect **was measured**. Its third finding is the previous head's own repair
failing in the way this branch family keeps failing — two rows were fixed and the other
five were asserted to be fine without being checked, and none of the five was.

## The review, verbatim

## Findings

1. **P1 — The PR ledger records the wrong reviewed SHA.** The body says the four findings came from the review of `31e24cc…` and that the current head still needs review ([pr.md:91](/srv/worktrees/pr42-r2/pr.md:91)), yet all four ledger rows claim they were reviewed at `f59a218…` ([pr.md:148](/srv/worktrees/pr42-r2/pr.md:148)). One even cites the subsequently added review record as the finding location. This launders an unreviewed head through the mechanical evidence validator, contrary to MAINTAINING’s exact-SHA contract. At least three rows also use `—` for first-bad despite history establishing `31e24cc…`.

2. **P2 — The first review’s ledger-contract finding was narrowed, not fixed.** That finding said the seven actionable standards findings require canonical PR-ledger fields; the repaired ledger rewrites it as only “miss the enforcement map.” The seven rows remain four-column entries without severity, exact reviewed SHA/location, failure sequence, provenance, category, first-bad, prevention, or canonical disposition ([FINDINGS.md:163](/srv/worktrees/pr42-r2/reviews/FINDINGS.md:163)). The live PR ledger contains four different PR42 findings, not these seven. Sequence: the body validator checks four rows → all seven open findings bypass its evidence checks → `FINDINGS.md`, which describes itself as the union of per-PR ledgers, gains entries that never existed in a canonical per-PR ledger.

3. **P2 — The hash repair still falsely describes what the unchanged hashes cover.** All nine digest values reproduce, but the body’s claim that the other five cover documentation and decision together is false ([pr.md:151](/srv/worktrees/pr42-r2/pr.md:151)):

   - `authorized_root` lines 438–456 exclude its inline rationale at 435–437 and even `Ok(root)` at 457.
   - Question lines 820–831 exclude the doc comment at 819.
   - Answer lines 918–927 exclude the doc comment at 916–917.
   - Ownership lines 1563–1575 exclude the actual comparison at 1587–1595.
   - Container lines 324–332 exclude the doc comment at 316–323.

   Edit any excluded rationale—or the ownership comparison—and the digest still verifies while salvage reports the observation intact. The separate prior-review objection that W10.4 requires whole-file identity is also untouched: all seven rows still assert “Salvageable by hash per W10.4,” with no rejection or correction.

4. **P2 — The move and rollback story remains internally contradictory.** The summary says the findings “leave” the work-list ([pr.md:5](/srv/worktrees/pr42-r2/pr.md:5)); Scope admits that this tree’s work-list never contains them. More seriously, rollback says to revert every first-parent commit newest-first and then claims the review record remains as a durable copy ([pr.md:113](/srv/worktrees/pr42-r2/pr.md:113)). The actual sequence is: revert `f59a218` → review record is deleted; revert `f5204bb` → repairs are undone; revert `31e24cc` → rows are deleted. The current tip then has neither. If “repository copy” instead includes Git history, the FINDINGS rows also survive historically, so singling out the review record still does not work.

5. **P2 — The hard cross-PR merge order is unsupported and invokes the wrong authority model.** The body supplies no exact #40/#41 heads or durable evidence for their contents, file sets, triage wording, or where the three records land ([pr.md:120](/srv/worktrees/pr42-r2/pr.md:120)). This tree proves only that those files are absent here. Moreover, [decisions/README.md:8](/srv/worktrees/pr42-r2/decisions/README.md:8) says decision records are history, not living authority; only `DESIGN.md` is. Sequence: #40 changes or omits one asserted record/DESIGN amendment → maintainer follows the stated hard order → #41 still lands an unsupported ruling although #42 claimed the order prevented that. The claim needs exact-head evidence and the controlling DESIGN delta, not categorical assertions about moving PRs.

What checked out: the exact diff matches `3e5212d…f59a218`; only the two advertised Markdown files change. All seven section citations quote real MUST clauses and land on the correct §8/§14 enforcement-map rows with defensible `review-only` status. The review payload is byte-identical to the saved original and carries the correct full `31e24cc…` SHA. The work-list contains none of the seven, the triage is absent, and no `src/**`, `DESIGN.md`, `CODING_STANDARDS.md`, or `decisions/**` file is edited.

VERDICT: CHANGES_REQUIRED