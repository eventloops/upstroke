# 2026-08-31 — first-parent review coverage map, `76b6a784..50ed8c86`

A mechanical map of every commit in the G2 checkpoint candidate to the review
that covers it. Produced as part of the candidate assembly, at candidate head
`50ed8c86ec60164011bfd393066c4c3696d3865b`, against `master` at
`76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`.

**This map does not claim that any reviewer reread the candidate diff.** It
claims something narrower and checkable: that every commit in the range belongs
to exactly one first-parent unit, and that each unit is accounted for by a named
class of coverage — or is named as residue. Coverage of a unit is coverage as it
was given at the time, not coverage of the merged whole.

## The partition

```
git rev-list --count               76b6a784..50ed8c86   ->  418
git rev-list --count --first-parent 76b6a784..50ed8c86  ->   66
```

Each first-parent unit owns the commits reachable from it but not from its first
parent and not from `master`. The 66 unit sizes sum to 418 exactly, so the
partition is total and disjoint — no commit is counted twice and none is missed.

| Class | Units | Commits | What covers it |
|---|---:|---:|---|
| **S** — slice pull request | 33 | 369 | The per-head slice ceremony of `decisions/2026-08-21-stacked-slice-prs.md`: CI, PR policy, and a single-reviewer review of each head. The verdict lives in the pull-request body, which is **not** in this repository; `reviews/FINDINGS.md` §24–§34 and the `reviews/*-frontier-review-*.md` records carry the in-tree part for the subset that has one |
| **B** — bridge / master-forward pull request | 2 | 16 | Same ceremony. PR #79 is the bridge whose reviewed head `3348ce8` has a tree *identical* to the merge `50ed8c86`; PR #34 is the earlier `bridge/master-merge` |
| **M** — master-forward merge (non-PR) | 5 | 6 | Content that arrived from `master`, where it was already reviewed and attested before it was merged forward. No new integration-branch code |
| **D** — docs / reviews exemption | 12 | 13 | The unit's whole diff is confined to `reviews/`, `decisions/`, `proposals/`, `docs/`, `acceptance/`, a root `*.md`, or the two ignore files. Permitted as a docs/reviews exemption; `decisions/2026-08-20-review-invalidation-scope.md` is the record that makes the `reviews/FINDINGS.md` case explicit |
| **P** — pre-PR-regime direct commit | 14 | 14 | Landed on the integration branch before slices became pull requests. Coverage is the in-branch packet review of its era, cited in the commit's own message where it exists — and **absent for seven of them**, which is this map's residue |

33 + 2 + 5 + 12 + 14 = **66 units**. 369 + 16 + 6 + 13 + 14 = **418 commits**.

## Genuinely unmapped residue

Seven **P** units carry no pull-request body, no `reviews/` record, no
`reviews/FINDINGS.md` section naming them, and no review reference in their own
commit message. A sha search across `reviews/` and `decisions/` returns nothing
for any of them.

| Unit | Commit | What it is |
|---|---|---|
| 36 | `16a8036` | `rename: tactus -> upstroke on the integration branch` — mechanical, 132 files, produced by `scripts/rename-tactus-to-upstroke.sh` |
| 49 | `2746ed6` | `fix(effects): record libc::pipe2 as the Linux-only denial it is` — 2 files |
| 56 | `510ce2a` | `fix(runner): make three PR4 tests independent of the machine that runs them` — 4 files, tests only |
| 63 | `73cd006` | `feat(config): strict [engine] limits validated before any lock` — 7 files |
| 64 | `eacdbda` | `refactor(engine): rewire test imports and fix facade doc link` — 2 files |
| 65 | `651c719` | `refactor(engine): split engine.rs into production modules` — 8 files |
| 66 | `df05503` | `refactor(engine): extract inline tests` — 2 files |

**What this is and is not.** It is not a claim that these commits are unreviewed:
five of the seven are refactors, test-independence fixes, or a scripted rename,
and the config unit (`73cd006`) is the one carrying real production logic. It is
a claim that **the repository does not record who reviewed them**, which is the
thing a checkpoint audit exists to surface. `73cd006` is the one a panel should
look at directly, because its subject — `[engine]` limits validated before any
lock — is the same seam the `max_parallel` refusal in
`reviews/2026-08-31-g2-gate-report.md` depends on.

The other seven **P** units do cite their review in the commit message:
`7a83e69`, `d6c82fd`, `ad2ea84`, `f6e7d88`, `1a9cb20`, `bc07139`, `ae9e9da`.

## Reconciliation against the P1A census baseline

The P1A read-only census was taken at `0bd12cb`, before the bridge. Re-derived at
`50ed8c86`:

| Quantity | P1A at `0bd12cb` | Re-derived at `50ed8c86` | Delta |
|---|---:|---:|---|
| Commits in range | 416 | 418 | +2 — the bridge |
| First-parent units | 65 | 66 | +1 — the PR #79 merge |
| PR units | 34 | 35 | +1 |
| Commits in PR units | 383 | 385 | +2 |
| Non-PR units | 31 | 31 | 0 |
| Commits in non-PR units | 33 | 33 | 0 |

The two added commits are the PR #79 merge `50ed8c86` and its reviewed head
`3348ce8`. Both sit inside one PR unit, which is why the non-PR figures do not
move. The baseline is therefore sound and its numbers carry forward with the
stated delta — nothing in the census needed re-taking.

*(This table's PR / non-PR split is the P1A partition, kept so the two can be
compared directly. The five-class split above is a refinement of the same 66
units: classes **S** and **B** together are the 35 PR units, and **M**, **D**
and **P** together are the 31 non-PR units.)*

## Every unit

Newest first. **n** is the number of range commits the unit owns.

| # | First-parent | Kind | n | Class | Subject |
|---|---|---|---:|:---:|---|
| 1 | `50ed8c8` | PR #79 | 2 | **B** | Merge pull request #79 from eventloops/promotion/master- |
| 2 | `0bd12cb` | PR #78 | 7 | **S** | Merge pull request #78 from eventloops/standards/w10-emi |
| 3 | `82874ef` | PR #77 | 6 | **S** | Merge pull request #77 from eventloops/standards/w10-run |
| 4 | `f7fe2c3` | PR #75 | 2 | **S** | Merge pull request #75 from eventloops/test/taskkind-all |
| 5 | `4a4a46a` | PR #73 | 8 | **S** | Merge pull request #73 from eventloops/standards/effects |
| 6 | `ce3a54c` | PR #71 | 3 | **S** | Merge pull request #71 from eventloops/standards/ulid-de |
| 7 | `3db8e5b` | PR #69 | 2 | **S** | Merge pull request #69 from eventloops/standards/w10-val |
| 8 | `5d81dc7` | PR #67 | 3 | **S** | Merge pull request #67 from eventloops/standards/w10-sta |
| 9 | `116276f` | PR #68 | 2 | **S** | Merge pull request #68 from eventloops/standards/w10-con |
| 10 | `afc284d` | PR #66 | 2 | **S** | Merge pull request #66 from eventloops/standards/w10-mar |
| 11 | `e03d32d` | PR #63 | 3 | **S** | Merge pull request #63 from eventloops/standards/w10-pre |
| 12 | `08ce471` | PR #62 | 2 | **S** | Merge pull request #62 from eventloops/standards/w10-top |
| 13 | `485020b` | PR #61 | 2 | **S** | Merge pull request #61 from eventloops/decomposition/eff |
| 14 | `413bfac` | PR #60 | 2 | **S** | Merge pull request #60 from eventloops/decomposition/eff |
| 15 | `c251ffe` | PR #59 | 2 | **S** | Merge pull request #59 from eventloops/decomposition/eff |
| 16 | `9fffaa0` | PR #58 | 2 | **S** | Merge pull request #58 from eventloops/decomposition/eff |
| 17 | `8d817db` | PR #56 | 2 | **S** | Merge pull request #56 from eventloops/decomposition/eff |
| 18 | `5a0b7da` | PR #54 | 2 | **S** | Merge pull request #54 from eventloops/decomposition/eff |
| 19 | `ec8c6de` | PR #53 | 3 | **S** | Merge pull request #53 from eventloops/standards/phase-b |
| 20 | `1fba576` | PR #52 | 3 | **S** | Merge pull request #52 from eventloops/standards/phase-b |
| 21 | `6e869e6` | PR #51 | 4 | **S** | Merge pull request #51 from eventloops/standards/phase-b |
| 22 | `859fa6e` | PR #50 | 3 | **S** | Merge pull request #50 from eventloops/standards/phase-b |
| 23 | `68c7f4a` | PR #49 | 6 | **S** | Merge pull request #49 from eventloops/findings/ci-yaml- |
| 24 | `982474a` | PR #48 | 8 | **S** | Merge pull request #48 from eventloops/findings/g2-w1-fo |
| 25 | `bc30d7c` | PR #47 | 9 | **S** | Merge pull request #47 from eventloops/findings/runner-p |
| 26 | `fc717cf` | PR #46 | 4 | **S** | Merge pull request #46 from eventloops/findings/workspac |
| 27 | `ff86d29` | PR #45 | 3 | **S** | Merge pull request #45 from eventloops/reviews/consolida |
| 28 | `3b9ea69` | PR #43 | 16 | **S** | Merge pull request #43 from eventloops/evidence/macos-pr |
| 29 | `71a85e2` | PR #41 | 14 | **S** | Merge pull request #41 from eventloops/standards/pr7-wor |
| 30 | `ae05e53` | PR #42 | 13 | **S** | Merge pull request #42 from eventloops/standards/trust-b |
| 31 | `a23170d` | PR #40 | 26 | **S** | Merge pull request #40 from eventloops/docs/g2-charter-r |
| 32 | `3e5212d` | PR #34 | 14 | **B** | Merge pull request #34 from eventloops/bridge/master-mer |
| 33 | `26c6e6c` | PR #31 | 170 | **S** | Merge pull request #31 from eventloops/slice/pr7 |
| 34 | `615597c` | non-PR | 1 | **M** | Merge master into codex/parallelism-design |
| 35 | `5196c0b` | non-PR | 1 | **M** | Merge master into codex/parallelism-design |
| 36 | `16a8036` | non-PR | 1 | **P** | rename: tactus -> upstroke on the integration branch |
| 37 | `7c4f974` | PR #28 | 7 | **S** | Merge pull request #28 from eventloops/fix/win-sharing-v |
| 38 | `150c2f7` | PR #27 | 28 | **S** | Merge pull request #27 from eventloops/slice/pr6 |
| 39 | `9f1be2b` | non-PR | 1 | **M** | Merge master into codex/parallelism-design |
| 40 | `25a807e` | non-PR | 1 | **D** | docs(reviews): name which provider the exhaustion events |
| 41 | `c3039f4` | non-PR | 1 | **D** | docs(reviews): capacity binds as a rate, not a volume |
| 42 | `302c841` | non-PR | 1 | **D** | docs(reviews): close the catalogue re-measurement at 190 |
| 43 | `5b7417f` | non-PR | 1 | **D** | docs(reviews): file the catalogue re-measurement's findi |
| 44 | `781bff5` | non-PR | 1 | **D** | docs(reviews): give the capacity row its distribution |
| 45 | `9d75632` | non-PR | 2 | **D** | Merge docs/capacity-forward-constraint |
| 46 | `bf15b52` | non-PR | 1 | **D** | docs(reviews): give the carried worktree-residue rows an |
| 47 | `7c4163f` | non-PR | 1 | **D** | docs(reviews): record that a history rewrite orphans the |
| 48 | `4294a16` | non-PR | 1 | **M** | Merge master into codex/parallelism-design |
| 49 | `2746ed6` | non-PR | 1 | **P** | fix(effects): record libc::pipe2 as the Linux-only denia |
| 50 | `7a83e69` | non-PR | 1 | **P** | feat(effects): route every filesystem effect through a f |
| 51 | `20d44a4` | non-PR | 1 | **D** | docs(reviews): record the owner ruling that the frozen f |
| 52 | `d6c82fd` | non-PR | 1 | **P** | fix(runner): pin what the supervision tests assert, and  |
| 53 | `ad2ea84` | non-PR | 1 | **P** | fix(runner): make the CLI ambient-latch test immune to i |
| 54 | `f6e7d88` | non-PR | 1 | **P** | fix(runner): close the three gaps the frontier review of |
| 55 | `31b2b6f` | non-PR | 1 | **D** | docs(reviews): record PR4's CI attestation and the macOS |
| 56 | `510ce2a` | non-PR | 1 | **P** | fix(runner): make three PR4 tests independent of the mac |
| 57 | `1a9cb20` | non-PR | 1 | **P** | feat(runner): CommandSpec, host Runner, typed Invocation |
| 58 | `753790d` | non-PR | 1 | **D** | docs(reviews): close the Windows known-unknown with CI e |
| 59 | `59a6830` | non-PR | 1 | **D** | docs(process): record the review-effort decision and a s |
| 60 | `e4b0508` | non-PR | 2 | **M** | Merge remote-tracking branch 'origin/master' into codex/ |
| 61 | `bc07139` | non-PR | 1 | **P** | feat(topology): schema-4 vocabulary, header-first decode |
| 62 | `ae9e9da` | non-PR | 1 | **P** | feat(topology): TaskKey registry with legacy projection  |
| 63 | `73cd006` | non-PR | 1 | **P** | feat(config): strict [engine] limits validated before an |
| 64 | `eacdbda` | non-PR | 1 | **P** | refactor(engine): rewire test imports and fix facade doc |
| 65 | `651c719` | non-PR | 1 | **P** | refactor(engine): split engine.rs into production module |
| 66 | `df05503` | non-PR | 1 | **P** | refactor(engine): extract inline tests |
