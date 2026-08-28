# PR #41 — frontier review record, `4fabf5a`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, three findings** |
| **Reviewed SHA** | `4fabf5a889ec14ed36ff9b8fcf11f48add235fa8` |
| Pull request | eventloops/upstroke#41, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 41`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed prior review records, and the pull request's title and body |

The fourth review of #41, and the first whose count fell to three. Its first finding is a
repair that was written into a commit message and never reached the file: a script that
edited the triage raised an error before writing, so the ledger claimed a correction the
tree did not contain. Its second is the recorded class again — adding the missing
membership row left the heading, the mode count, the class list and the workstream
discussion all describing the table as it was before.

## The review, verbatim

Changes are required.

1. **P2 — A prior finding is marked fixed without changing the defective text.** The ledger says the escalation consequences were restated and now require a two-sided, backward-compatible representation ([ledger disposition](/srv/worktrees/pr41-r4/pr.md:146)). But the triage still says a “yes” rejects all of class A plus `scaffold.rs` ([triage](/srv/worktrees/pr41-r4/reviews/2026-08-28-pr7-standards-triage.md:238)) and still describes repair as a writer-side conversion ([triage](/srv/worktrees/pr41-r4/reviews/2026-08-28-pr7-standards-triage.md:379)). Those passages are unchanged from `0d994b7`.

   Concrete harm: an implementer follows that advice and encodes Unix byte `0x80` as `%80` at the writer. Recovery still calls `PathBuf::from(&started.private_dir)` ([recover.rs](/srv/worktrees/pr41-r4/src/engine/topology/recover.rs:335)), so it opens the literal `%80` path rather than the original path. Adding an untagged decoder instead reinterprets historical paths genuinely named `%80`. The claimed repair therefore prevents neither replay failure nor compatibility damage.

2. **P2 — Adding the omitted membership row left multiple sibling claims stale.** The table now has fifteen members, but its heading still says fourteen ([triage](/srv/worktrees/pr41-r4/reviews/2026-08-28-pr7-standards-triage.md:140)); the prose says “Three failure modes” before defining A through D ([triage](/srv/worktrees/pr41-r4/reviews/2026-08-28-pr7-standards-triage.md:173)); later prose still says fourteen rows ([triage](/srv/worktrees/pr41-r4/reviews/2026-08-28-pr7-standards-triage.md:246)); and the W4 discussion repeats fourteen and the obsolete writer-conversion remedy. The class-C rationale list also retains only `156, 237, 267`, omitting newly classified `149` and `258`. Older ledger dispositions likewise still claim “all fourteen” and three modes. This is exactly the repair-one-quoted-site/leave-siblings-stale failure pattern the round instructions warned about.

3. **P2 — The advertised structural deletion did not happen consistently.** Outside the ledger, with no generated block marked, the body retains prohibited recomputable quantities and mutable state: “applied wrongly three times” ([pr.md](/srv/worktrees/pr41-r4/pr.md:16)), “Two rows” ([pr.md](/srv/worktrees/pr41-r4/pr.md:43)), the current-head gate conclusion ([pr.md](/srv/worktrees/pr41-r4/pr.md:76)), and “the most recent round” chronology ([pr.md](/srv/worktrees/pr41-r4/pr.md:89)). This directly contradicts the claim that every recomputable fact was deleted and fails question B. The deleted file/commit lists and counts remain recoverable from the diff, log, and committed review records, so I found no question-A loss.

What checked out: `pr.diff` exactly matches `3e5212d…4fabf5a`; only Markdown under `reviews/` changes. No source, gate, workflow, configuration, `DESIGN.md`, or immutable decision record is touched. All seventeen ledger locations resolve at their bound SHAs, and the newest row additions use the correct reviewed SHA and prior IDs. The three reclassifications, Claude membership addition, and `exec_streams` table removal are present; the claimed escalation-consequence repair is not.

VERDICT: CHANGES_REQUIRED