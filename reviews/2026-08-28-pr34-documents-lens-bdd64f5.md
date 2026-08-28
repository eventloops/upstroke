# PR #34 — frontier review record, `bdd64f5`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `bdd64f5` |
| Pull request | eventloops/upstroke#34, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 34`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| Inputs | the bridge diff, the tree at that sha, and the lens's own scope: the four documents, both indexes, the `DESIGN.md` §21 edit, and the charter-versus-PR31 surface |

One of five independent lenses at `bdd64f5`. Committed here because #40 answers its four findings and cites it as review evidence; the bridge's own PASS record (`2026-08-28-pr34-bridge-frontier-review.md`) records that the lenses ran but does not carry their verdicts, and a body may not cite a build-box path as its durable link.

## The review, verbatim

1. The charter lands with a known-false account of PR #31. It says reader accretion stops and the eleventh reader is the last outside the pass, then retroactively blesses those eleven and “nothing else” ([charter](/srv/worktrees/pr34-documents/decisions/2026-08-24-pr3-layer-freeze-charter.md:21)). But PR #31 subsequently added `open_no_attempt`; its ledger explicitly calls it the “twelfth fold reader” ([FINDINGS.md](/srv/worktrees/pr34-documents/reviews/FINDINGS.md:416)), and it remains in the tree ([fold.rs](/srv/worktrees/pr34-documents/src/topology/fold.rs:1012)). Sequence: the no-more-readers adjudication landed at `40c5d89`; `ffcc74a` added reader twelve; PR #31 merged; only afterward did `5763fe3` land the unchanged charter. The generic Class-A allowance does not reconcile the explicit temporary moratorium.

   The B/C ceremony otherwise matches the final taxonomy: per-instance owner approval is a permitted Class B path, and the durable-feedback Class C change has its decision record. But those approvals independently authorize the instances; PR #31 could not have operated under a repository charter that landed afterward. The accounting is also wrong: [pr.md](/srv/worktrees/pr34-documents/pr.md:125) says three Class-B approvals, while §3 has four headings—deferrals, ladder position, candidate-tree verification, and sole successful settlement ([FINDINGS.md](/srv/worktrees/pr34-documents/reviews/FINDINGS.md:175)). Finally, the proposal says Class A is the only remaining outside-pass route ([proposal](/srv/worktrees/pr34-documents/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:205)), contradicting the charter’s continuing B/C exception routes. §22e shows that PR #31 eventually stopped and carried further work to G2; it does not cure these contradictory rules.

2. The `OsString` decision violates the sole-authority contract. The contract requires a spec-changing decision to update `DESIGN.md` at decision time ([decisions/README.md](/srv/worktrees/pr34-documents/decisions/README.md:8)). This PR lands a verdict that `CommandSpec.program` widens to `OsString`, but deliberately postpones that edit until W4 ([record](/srv/worktrees/pr34-documents/decisions/2026-08-25-commandspec-program-osstring.md:3)). Consequently, the sole authority still says `program: String` ([DESIGN.md](/srv/worktrees/pr34-documents/DESIGN.md:222)), while the coding standard still calls the matter unresolved “until that ruling” ([CODING_STANDARDS.md](/srv/worktrees/pr34-documents/CODING_STANDARDS.md:36)).

   Concrete harm: after this merges, one W4 implementer follows the sole living authority and preserves the Unicode refusal; another follows the new immutable record/index and implements `OsString`. Each can correctly accuse the other of violating a governing document. Either the ruling is effective now, in which case `DESIGN.md` is owed now, or it is deferred until W4, in which case the body and index overstate it as resolved. The §21 paragraph is faithful to the charter’s sequencing consequence, but it is not the only compressed design edit owed by the four records.

3. The checkpoint record does not satisfy the decision-folder form. [decisions/README.md](/srv/worktrees/pr34-documents/decisions/README.md:3) requires measured versus assumed claims to be named explicitly. The checkpoint record asserts that review quality degrades “superlinearly” and that a future v0.2-complete diff would be “nearly double” the G2 diff ([record](/srv/worktrees/pr34-documents/decisions/2026-08-25-checkpoint-merges.md:34)) without measurement or labeling either claim as assumed. The other two new records do provide explicit measured/assumed sections.

4. The body overstates its evidence. `test-docs-consistency.sh` declares its exact C1–C4 scope and reads `CLAUDE.md`, `CONTRIBUTING.md`, workflow triggers, Cargo MSRV, and gate inventory ([script](/srv/worktrees/pr34-documents/.github/scripts/test-docs-consistency.sh:5)); it never examines these four documents, either index, or the `DESIGN.md` edit. It passes while every contradiction above remains, so [pr.md’s claim](/srv/worktrees/pr34-documents/pr.md:76) that it reads the documents edited here is false as evidence for this document landing.

   Likewise, the diff adds all three records from `/dev/null`; it does not show any `Status: DRAFT` line being removed. The final no-status form is allowed, but the body’s stronger assertion that “a landed record carries none” is not in the README and is contradicted by seven existing landed decision records with `Status:` blocks.

The index positions themselves are correct: decisions are oldest-first and proposals newest-first. The proposal also contains all required `Status`, `Target`, `Filed`, and `Review` fields; `Review: Unreviewed` is allowed because review and status are explicitly orthogonal.

COVERAGE
- read in full: `pr.md`; `decisions/README.md`; `proposals/README.md`; the three new decision records; `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md`; `decisions/2026-08-26-durable-retry-feedback.md`; `.github/scripts/test-docs-consistency.sh`
- read in part: `pr.diff` document/index/DESIGN hunks and file inventory; `DESIGN.md` §8’s `CommandSpec` and the relevant §21 sequence; `CODING_STANDARDS.md` §1 conflict block; `reviews/FINDINGS.md` §§3 and 22e in full plus the relevant §2 rows; `reviews/2026-08-25-pr7-standing-questions.md` Q1; `reviews/2026-08-25-pr7-g2-evidence.md` frozen-change disclosure; `src/topology/fold.rs` for the twelfth reader; existing `decisions/*.md` status-line census; relevant Git ancestry and commit dates
- NOT examined: non-document source, lint-resolution, and infrastructure hunks outside this lens; no assigned document/status/index/§21/charter-versus-PR31 surface was left unexamined

VERDICT: CHANGES_REQUIRED==========================================

