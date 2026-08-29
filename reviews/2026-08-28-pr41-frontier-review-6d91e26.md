# PR #41 — frontier review record, `6d91e26`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `6d91e2615b319d643b59c1be2ff2a19d28c22af9` |
| Pull request | eventloops/upstroke#41, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 41`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed prior review records, and the pull request's title and body |

The fifth review of #41. Its fourth finding is the substantive one and it corrects a repair
rather than a slip: replacing a wrong remedy with a single right-sounding one and applying it
to a whole class is its own error. A two-sided, backward-compatible representation is right
for a value that is persisted and read back and wrong for one used at the moment of
hand-off — the executable probe builds its candidate locally, and an agent CLI cannot decode
a representation private to this engine.

Its first three findings are the seat's: a deletion rule applied by generalising the
offending sentences rather than removing them, a guard claiming a checker that is not in the
repository, and a count repair scoped to where the reviewer pointed while the ledger's own
dispositions kept the totals.

## The review, verbatim

Changes are required at exact head `6d91e2615b319d643b59c1be2ff2a19d28c22af9`.

1. **P2 — The structural deletion still did not happen.** The body merely generalized or renamed several prohibited facts: “three times” became “more than once” ([pr.md:16](/srv/worktrees/pr41-r5/pr.md:16)); it still says “one site” ([pr.md:22](/srv/worktrees/pr41-r5/pr.md:22)); both duplicated deletion paragraphs retain the chronology “in every round” ([pr.md:30](/srv/worktrees/pr41-r5/pr.md:30)); “most recent round” became “most recent review” ([pr.md:104](/srv/worktrees/pr41-r5/pr.md:104)). Most decisively, the supposedly removed current-head outcome remains verbatim: “The hash gate rejected nothing” ([pr.md:63](/srv/worktrees/pr41-r5/pr.md:63)), contradicting the later “practice, not its outcome” paragraph. Other mutable conclusions survive at lines 56–61, 80–85, 91–96, 119–120, and 134–135. There is no generated block covering them.

   Concrete failure: a subsequent repair changes a filed quote or digest so the hash gate rejects it; the unchanged body still says nothing was rejected and every row relocated, so a maintainer accepts a filing whose salvage check now fails.

2. **P2 — The new ledger row for that finding has no valid guard or location.** `PR41-DELETION-APPLIED-TO-DIGITS-ONLY` claims “a checker enforces” number-words, conclusions, chronologies, and branch state ([pr.md:161](/srv/worktrees/pr41-r5/pr.md:161)). No checker changed in the exact diff, and the surviving text above demonstrates that any external checker did not enforce the property. Its bound location, [MAINTAINING.md:33](/srv/worktrees/pr41-r5/MAINTAINING.md:33), concerns recording a passing review’s metadata; it contains no obligation about deleting recomputable facts. This is not a complaint that the body-finding convention remains with the owner—the row simply does not disclose an obligation anchor or cite a relevant one.

3. **P2 — The count repair omits a part of the finding it purports to record.** The prior review explicitly included the older ledger dispositions among the stale siblings. They remain: [pr.md:144](/srv/worktrees/pr41-r5/pr.md:144) and [pr.md:150](/srv/worktrees/pr41-r5/pr.md:150) still claim “all fourteen rows” and “three failure modes”, while the triage membership table has fifteen members and modes A–D. The new `PR41-COUNTS-OUTLIVED-THE-ROW-THAT-MOVED-THEM` row narrows the prior finding to triage prose and marks it fixed, making the ledger part of the original finding disappear. Additionally, `PR41-REPAIR-NEVER-REACHED-THE-FILE` labels a recurrence caused by a failed repair as `introduced_by_feature`, although the repository defines that case as `fix_regression`.

4. **P2 — The repair overgeneralizes a durable-format remedy to sites with no durable format.** The triage now says every lossy-path member’s repair is a “two-sided, backward-compatible representation” ([triage:398](/srv/worktrees/pr41-r5/reviews/2026-08-28-pr7-standards-triage.md:398)). That follows for persisted fields reconstructed by recovery, but not for class-C’s local executable probe: [gates.rs:465](/srv/worktrees/pr41-r5/src/gates.rs:465) constructs a candidate path locally through `display()`. It has no writer, persisted representation, historical encoding, or second consumer; an OS-native `OsString` append fixes it locally. Treating it as a versioned design decision can defer that repair while a valid executable under a non-UTF-8 path remains undetected. Likewise, passing a percent-encoded settings path to Claude would name literal `%80`; the external CLI cannot decode Upstroke’s private representation.

Nothing necessary was lost through deletion: file and commit scope, chronology, CI state, and review history remain recoverable from the diff, Git, the API, and committed review records. `pr.diff` exactly matches the base-to-head Git diff; only Markdown under `reviews/` changes. No `src/**`, `DESIGN.md`, decision, gate, workflow, or configuration file is touched, so the unwrap/`anyhow`/Windows-path and immutable-decision rules are not directly violated. Every ledger location resolves mechanically at its bound SHA; the defects above are semantic.

VERDICT: CHANGES_REQUIRED