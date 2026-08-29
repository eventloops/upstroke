# PR #40 — frontier review record, `db3e855`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `db3e85581de9a1e117950c1829883fd96bd21d95` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, after re-running one macOS leg that matched the recorded `PR7-MACOS-PROCESS-GROUP-FLAKE` fingerprint exactly; attempt 1 is preserved outside the workspace |
| Inputs | the diff, the tree at that sha, the eight committed prior review records, and the pull request's title and body |

**The first review to confirm the withdrawal.** It verified independently that
`Invocation::at` and both its call sites are test-only, that every production adapter calls
`Invocation::named` with a fixed bare name, that `spec` refuses only when `to_str()` fails,
and that the host runner keeps `PATH` as `OsStr`/`PathBuf` through resolution without
writing it back — and it found the superseding standing-ledger row accurate.

All four findings are execution. The one worth carrying is the first: the sweep that
followed the withdrawal matched the **vocabulary** of the change — the type name, the record
name — and so passed over the charter's errata list, which orders an erratum for the field's
*shape* and names neither.

## The review, verbatim

## Findings

1. **P1 — the withdrawal left binding siblings behind.** The charter still orders a packet erratum for “the `CommandSpec` shape” ([charter:184](/srv/worktrees/pr40-r13/decisions/2026-08-24-pr3-layer-freeze-charter.md:184)), directly contradicting the proposal’s statement that this erratum is no longer owed ([proposal:288](/srv/worktrees/pr40-r13/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:288)). The proposal also still classifies W1–W4 as behaviour changes, each requiring a mutation witness ([proposal:42](/srv/worktrees/pr40-r13/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:42)), despite declaring W4 empty ([proposal:91](/srv/worktrees/pr40-r13/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:91)). Sequence: the owner follows the binding charter → writes a `CommandSpec` packet-shape erratum → W4 either widens the field despite the withdrawal or blocks because the proposal forbids it. This is the silent scope widening the PR claims to remove.

2. **P1 — the new ledger rows repeat the wrong-SHA and wrong-location defect.** The committed review says it examined `1de913109e72248383fcced5271bf3d8556f01ae` ([review:6](/srv/worktrees/pr40-r13/reviews/2026-08-28-pr40-frontier-review-1de9131.md:6)), but all four corresponding `PR40J-*` rows bind to `e8c8735…` ([pr.md:321](/srv/worktrees/pr40-r13/pr.md:321)). Git shows `e8c8735` only added that review record; it was the answering head, not the reviewed head. Moreover, `PR40J-OSSTRING-RATIONALE-HAS-NO-PRODUCTION-ROUTE` anchors at the correct source evidence, `src/agent/bin.rs:236`, rather than the defective decision rationale at `decisions/2026-08-25-commandspec-program-osstring.md:50/73` in the reviewed tree ([pr.md:324](/srv/worktrees/pr40-r13/pr.md:324)). An auditor follows the row and sees correct code, not the false claim. This contradicts the adjacent guard’s assertion that every row now binds to the reviewed SHA and anchors at the defective text.

3. **P2 — the standards defect is routed in prose but closed dishonestly.** `CODING_STANDARDS.md` remains inaccurate, W4 explicitly carries no deletion, and no successor PR, owner, or exit criterion owns the separate motion. Nevertheless `PR40K-STANDARDS-SECTION-1-NOW-INACCURATE` is marked `fixed` ([pr.md:325](/srv/worktrees/pr40-r13/pr.md:325)). Sequence: this PR merges → W4 completes as an empty workstream → the fixed row is skipped during audit → §1 continues directing readers to an open question and venue that no longer exist. Keeping `CODING_STANDARDS.md` out of this PR is correct, but the row must remain deferred/open with an explicit owner and successor venue until the master repair lands.

4. **P2 — deletion was again replaced by equivalent live assertions.** Scope still asserts mutable sibling-branch placement, non-overlap, and merge order ([pr.md:165](/srv/worktrees/pr40-r13/pr.md:165)), despite the `PR40F-MERGE-ORDER-UNPINNED` guard claiming this PR asserts nothing about another branch. Validation also attaches a local outcome—“It was checked, and it is this worktree’s” ([pr.md:181](/srv/worktrees/pr40-r13/pr.md:181))—while `PR40J-DELETION-DONE-BY-PARAPHRASE` says the gate paragraph has no outcome attached. A sibling branch can move its append or merge first, making the non-overlap claim false while this body remains unchanged. These survivors also disprove the body’s claim that every recomputable fact lives only in the generated block.

The withdrawal evidence itself is sound: `Invocation::at` and both call sites are test-only; all production adapters call `Invocation::named` with fixed bare names; `spec` refuses only when `PathBuf::to_str()` fails; and the host runner keeps `PATH` as `OsStr`/`PathBuf` through resolution without writing it back into `CommandSpec.program`. The superseding standing-ledger row is adjacent, leaves the original untouched, and accurately says “not reproducible in production.”

The generated repository figures also recompute exactly: 17 files/1040 insertions, 19 first-parent commits, 31 total non-merge commits, 12 side commits, and 48 findings across 9 records. All 67 ledger locations exist and are in range at their stated SHA; the defect above is semantic binding/aboutness. No source, `src/topology/**`, `CODING_STANDARDS.md`, or `DESIGN.md:222` change is present, so the panic, `anyhow`, and path-portability rules are otherwise not implicated.

VERDICT: CHANGES_REQUIRED