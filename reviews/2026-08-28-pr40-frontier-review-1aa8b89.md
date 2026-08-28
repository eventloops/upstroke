# PR #40 — frontier review record, `1aa8b89`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, six findings** |
| **Reviewed SHA** | `1aa8b89626f5e19ceb7d1a0cbb2a086bd025c534` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name — `CI` run `33169307894`, `Pull request policy` run `33169307848`, both first-attempt successes |
| Inputs | the diff, the tree at that sha, and five prior review records — the four then committed under `reviews/`, plus `c3e5b20`'s placed in the working directory as `prior-c3e5b20.md` because no head had yet committed it |

The fifth review of #40, and the one that named the custody rule this branch now
follows: **the review of a head is committed by whichever head answers it.** Its first
finding is that `1aa8b89` answers `c3e5b20` without committing `c3e5b20`'s record, so
the only copy of the review driving the repairs was an untracked file in a worktree.
Findings 4 and 5 are the recorded class again — the previous round repaired W8's and
W10's quoted instructions and left the exit criterion and the workstream synopsis
describing the superseded world.

## The review, verbatim

1. **The review history, title, and custody claim omit the review that produced this head.** The table totals 4+4+7+5=20 correctly, but omits the six-finding review of `c3e5b20`; the actual history is five completed rounds and 26 findings. Consequently, the [title](/srv/worktrees/pr40-r5/pr.md:1), [history](/srv/worktrees/pr40-r5/pr.md:194), and claim that every relied-upon review is committed under `reviews/` are false. The `c3e5b20` review exists only as untracked `prior-c3e5b20.md`, even though `1aa8b89` directly answers it. Sequence: this workspace is removed → the latest review disappears → the reasons for the head’s repairs cannot be durably verified, exactly the custody failure the body says it prevents. Under the prompt’s stated rulings, the five-ruling list also omits W8 and W10, making seven rather than five.

2. **The formal Scope still describes the old seven-file diff.** It says Git returns exactly seven paths, all under `decisions/`, `proposals/`, or `DESIGN.md` ([pr.md](/srv/worktrees/pr40-r5/pr.md:81)). Git returns 11 paths, including four under `reviews/`, as the Summary correctly says. A reviewer following Scope can omit those four records—the same class of files where this head had to repair a short reviewed SHA. The Summary prevents this from being wholly silent, but the PR does not do its Scope claim exactly.

3. **The commit counts remain stale in multiple siblings.** After `5763fe3`, `--first-parent --no-merges` returns nine commits: eight branch-owned plus `fb1d874`. The Summary says seven owned plus an eighth concurrent commit and omits `c3e5b20` ([pr.md](/srv/worktrees/pr40-r5/pr.md:12)). From `3e5212d`, the command printed beside that table returns ten non-merge commits. Conversely, the rollback section correctly says 9/21, while ledger row `PR40C-ROLLBACK-PULLS-THE-MERGED-SIDE` still says 8/20 ([pr.md](/srv/worktrees/pr40-r5/pr.md:428)). This directly refutes the row’s claim that the enumeration checks both commit counts.

4. **W8’s exit criterion still encodes the superseded first-application world.** W8 now emphatically requires re-measurement rather than application ([proposal](/srv/worktrees/pr40-r5/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:165)), but exit criterion 3 remains “zero unapplied entries and zero unexplained survivors” ([proposal](/srv/worktrees/pr40-r5/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:265)). Concrete failure: PR7’s application evidence already satisfies the “unapplied” wording → W1–W7 alter production behavior → a previously killed mutation now survives → W8 is skipped because the written criterion contains no exact-head re-measurement requirement → the pass claims completion without detecting the regression.

5. **W10 is still described the old way in its workstream summary.** The synopsis says “W10 is mechanical and runs last” ([proposal](/srv/worktrees/pr40-r5/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:40)), while W10.1 is already-landed history and W10.3 is a separate slice after this pass. An organizer following the ordered synopsis can either duplicate the completed standards work or pull the whole-tree sweep into this slice, contradicting the detailed instructions. The latest repair again fixed the quoted instruction while leaving its sibling summary stale.

6. **The ledger’s location disclosure is incomplete and does not satisfy the ledger contract.** `MAINTAINING.md` requires an exact finding file/line, not the “nearest true thing” ([MAINTAINING.md](/srv/worktrees/pr40-r5/MAINTAINING.md:64)). Even accepting the candid disclosure for the two named rollback rows, `PR40C-ROLLBACK-PULLS-THE-MERGED-SIDE` is a third body-only rollback finding pointing at the same unrelated `OsString` verdict but is not disclosed. Worse, `PR40C-CI-ACCOUNT-WRONG` points to the charter’s Class-A suspension ([pr.md](/srv/worktrees/pr40-r5/pr.md:431)), despite the relevant tracked location being [proc.rs:7960](/srv/worktrees/pr40-r5/src/agent/proc.rs:7960). That anchor shares no meaningful subject with the finding and should have been flagged by the claimed checker. The six-finding `c3e5b20` review also has no stable row for its ledger/anchor finding itself; only five `PR40C-*` rows were added.

I confirmed that `pr.diff` is byte-identical to Git’s exact 11-file/832-insertion diff; all four committed review bodies are faithful and carry full 40-character SHAs; the CI leaf-job count is nine; and the corrected exit-143/no-timeout account matches `proc.rs:7960–7966`. No non-Markdown file, `DESIGN.md:222`, `CODING_STANDARDS.md`, or `src/topology/**` is changed, so the panic, `anyhow`, and path-portability rules are not implicated. The absent checking tools are disclosed honestly as absent, but their claimed preventive value is unsupported and contradicted by this head.

VERDICT: CHANGES_REQUIRED