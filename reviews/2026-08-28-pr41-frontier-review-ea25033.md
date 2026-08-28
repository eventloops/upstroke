# PR #41 — frontier review record, `ea25033`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, six findings** |
| **Reviewed SHA** | `ea25033aab2a8cf24d20d077c1a2e0708c164465` |
| Pull request | eventloops/upstroke#41, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 41`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name — `CI` run `33157987233`, `Pull request policy` run `33157987228`, both first-attempt successes |
| Inputs | the diff, the tree at that sha, and the pull request's title and body |

The first review of #41. Three of its findings turned out to be corrections to the
owner's own rulings rather than to this seat's execution of them, and the owner amended
all three on the same day: the `scaffold.rs` row was struck on a ground the standard's
own opening lines refuse, the lossy-path rejection did not hold for `canonical_string`,
and the trust-boundary grounds named §8 where five of the seven rows cite §14. Its
second finding is independent of any ruling and is the most serious: a nine-site
unbounded-input class was removed from this file without being disposed of, while the
same file told the owner every escalated finding had been ruled on.

The record is committed here rather than left in a home directory on one build box.

## The review, verbatim

Changes are required. The tables are mechanically consistent, but the final triage and PR body are not.

1. The change silently expands from “carry undecided things to the owner” into making owner/design rulings. The body promises no resolution ([pr.md:35](/srv/worktrees/pr41-r1/pr.md:35)); the triage rejects findings, changes routing, rules W4’s scope, and declares that durable path identity stays `String` ([triage:51](/srv/worktrees/pr41-r1/reviews/2026-08-28-pr7-standards-triage.md:51), [triage:127](/srv/worktrees/pr41-r1/reviews/2026-08-28-pr7-standards-triage.md:127)). At this SHA, `CODING_STANDARDS.md` still calls the path question unresolved ([CODING_STANDARDS.md:38](/srv/worktrees/pr41-r1/CODING_STANDARDS.md:38)); the claimed W4 and checkpoint decision records do not exist, and `DESIGN.md` is unchanged. That makes a review document act as living product authority, contrary to [decisions/README.md:8](/srv/worktrees/pr41-r1/decisions/README.md:8), and violates the no-silent-scope-widening rule.

2. The claimed disposition count cannot be reconciled. The triage says all 28 escalated findings were ruled on ([triage:53](/srv/worktrees/pr41-r1/reviews/2026-08-28-pr7-standards-triage.md:53)), but accounts for only 13 lossy-path findings—12 rejected and one struck—and seven routed findings: at most 20. The final commit removed the earlier nine-finding unbounded-input class without giving it any disposition. Those include whole/incremental event-log reads ([worklist:214](/srv/worktrees/pr41-r1/reviews/2026-08-25-pr7-standards-worklist.md:214), [worklist:216](/srv/worktrees/pr41-r1/reviews/2026-08-25-pr7-standards-worklist.md:216)). Concrete failure: a multi-gigabyte or continuously growing persisted log drives `read_to_end` toward OOM or nontermination, yet the final triage tells the owner all 28 were handled while this class remains buried as sweep-only conformance work.

3. The lossy-path rejection is substantively unsupported and conflicts with the binding path rule. The cited `canonical_string` rationale concerns canonicalization failure; it does not address successful canonicalization followed by `to_string_lossy`. A run rooted beneath a valid Unix filename containing byte `0x80` records a replacement-character path and, after restart, reconstructs a different path. The triage rejects that class as compliant instead of preventing the failed resume. It also strikes the scaffold row solely because it is test-only ([triage:88](/srv/worktrees/pr41-r1/reviews/2026-08-28-pr7-standards-triage.md:88)), although the standard expressly applies to tests ([CODING_STANDARDS.md:3](/srv/worktrees/pr41-r1/CODING_STANDARDS.md:3)).

4. The routed rows do not carry SHA-256 digests. All seven values at [triage:108](/srv/worktrees/pr41-r1/reviews/2026-08-28-pr7-standards-triage.md:108) are 16 hexadecimal characters plus `…`, not 64-character SHA-256 values. The corresponding rows are also absent from `reviews/FINDINGS.md` at this head. Thus merging #41 without #42 leaves the present-tense routing assertion false and provides neither row IDs nor full hashes for the routed records. Calling #42 merely a soft dependency does not make the filing complete.

5. The measured “zero findings propose an edit” claim is false. One row explicitly says the representation “needs a native or dedicated target-path type” ([worklist:181](/srv/worktrees/pr41-r1/reviews/2026-08-25-pr7-standards-worklist.md:181)); another proposes a `PathHint`/path-pattern newtype ([worklist:243](/srv/worktrees/pr41-r1/reviews/2026-08-25-pr7-standards-worklist.md:243)).

6. The evidence claim is too strong. The body says hashes mechanically establish fidelity to the lenses ([pr.md:72](/srv/worktrees/pr41-r1/pr.md:72)), while its own validation correctly says they establish nothing about a lens’s judgement. Changing an observation or lens attribution while retaining a real file and digest passes the described controls. The exclusive assignment of the 15 source files having no findings and the claimed pre-lens eight-gate run are also unsupported in-repository because the manifest, logs, prompts, and `findings.json` are absent. Separately, the rollback text refers to “either commit” or “both,” but the exact PR range contains four commits.

What did check out: `pr.diff` exactly matches Git’s `3e5212d…ea25033` diff; only the two claimed Markdown files change; the tables contain exactly 101 and 220 rows; every lens total, the 26/12 citation counts, and the target’s 96 `.rs` count agree. All 321 work-list rows have full, distinct SHA-256 values, and every digest relocates to exactly one contiguous region at `3e5212d`. No source, `DESIGN.md`, or `decisions/` file is edited, so no unwrap/anyhow or immutable-record edit is introduced.

VERDICT: CHANGES_REQUIRED