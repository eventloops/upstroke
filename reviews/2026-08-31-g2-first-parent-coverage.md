# 2026-08-31 — first-parent review coverage map, `76b6a784..50ed8c86`

A mechanical map of every commit in the G2 pre-assembly baseline to the review
that covers it. Range is `master` at `76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`
to the baseline `50ed8c86ec60164011bfd393066c4c3696d3865b`.

**This map does not claim that any reviewer reread the diff.** It claims
something narrower and checkable: that every commit belongs to exactly one
first-parent unit, and that each unit either has **actual review or packet
evidence** or is named as residue.

**Revision, 2026-08-31 (second).** The PR #80 review found that the first
version of this map **invented a review exemption it had no authority for**: it
treated any diff confined to `decisions/`, `proposals/`, `docs/`, `acceptance/`,
a root `*.md` or an ignore file as exempt. `decisions/2026-08-20-review-invalidation-scope.md`
authorises **exactly one path — `reviews/FINDINGS.md`** — and says every other
path invalidates. That finding is correct. The exemption class is withdrawn and
every affected unit is reclassified below against actual evidence. **Residue
rises from 7 units to 18**, and `59a6830` — named by the review — is among them.

## The partition

```
git rev-list --count               76b6a784..50ed8c86   ->  418
git rev-list --count --first-parent 76b6a784..50ed8c86  ->   66
```

Each first-parent unit owns the commits reachable from it but not from its first
parent and not from `master`. The 66 unit sizes sum to 418 exactly, so the
partition is total and disjoint.

| Class | Units | Commits | The evidence, and its limit |
|---|---:|---:|---|
| **S** — slice pull request | 33 | 369 | The per-head ceremony of `decisions/2026-08-21-stacked-slice-prs.md`: CI, PR policy, and a single-reviewer review of each head. The verdict lives in the pull-request body, **outside this repository**; `reviews/FINDINGS.md` §24–§34 and the `reviews/*-frontier-review-*.md` records carry the in-tree part for the subset that has one |
| **B** — bridge pull request | 2 | 16 | Same ceremony. PR #79's reviewed head `3348ce8` has a tree **identical** to the merge `50ed8c86`; PR #34 is the earlier `bridge/master-merge` |
| **M** — master-forward merge, verified | 2 | 2 | Mechanically verified: **every** file in the unit's delta against its first parent is byte-identical to the blob at the merged-in parent, **and** that parent is an ancestor of `origin/master`. So the unit introduces no integration-branch-authored content, and what it introduces was attested under master's own required contexts |
| **X** — the authorised exemption | 11 | 12 | The unit's whole diff is **exactly `reviews/FINDINGS.md`** — verified per unit, not by prefix. This is the one path `decisions/2026-08-20-review-invalidation-scope.md` exempts |
| **R** — residue, no recorded review | 18 | 19 | No pull-request body, no `reviews/` record, no `reviews/FINDINGS.md` section naming the unit, and no review reference in its own commit message |

33 + 2 + 2 + 11 + 18 = **66 units**. 369 + 16 + 2 + 12 + 19 = **418 commits**.

## Residue — 18 units, 19 commits

Three groups. None of them is a claim that the code is bad; all of them are a
claim that **the repository does not record who reviewed it**.

**Group 1 — pre-PR-regime direct commits (14 units, 14 commits).** Landed on the
integration branch before slices became pull requests.

| Unit | Commit | What it is |
|---|---|---|
| 36 | `16a8036` | `rename: tactus -> upstroke on the integration branch` — mechanical, 131 non-exempt files, produced by `scripts/rename-tactus-to-upstroke.sh` |
| 49 | `2746ed6` | `fix(effects): record libc::pipe2 as the Linux-only denial it is` |
| 50 | `7a83e69` | `feat(effects): route every filesystem effect through a funnel that takes a site` — 46 non-exempt files |
| 52 | `d6c82fd` | `fix(runner): pin what the supervision tests assert` |
| 53 | `ad2ea84` | `fix(runner): make the CLI ambient-latch test immune to its own siblings` |
| 54 | `f6e7d88` | `fix(runner): close the three gaps the frontier review of b1864dd found` |
| 56 | `510ce2a` | `fix(runner): make three PR4 tests independent of the machine that runs them` |
| 57 | `1a9cb20` | `feat(runner): CommandSpec, host Runner, typed InvocationId, …` — 23 non-exempt files |
| 61 | `bc07139` | `feat(topology): schema-4 vocabulary, header-first decoder, checked fold, fault-seam framework` |
| 62 | `ae9e9da` | `feat(topology): TaskKey registry with legacy projection parity` |
| 63 | `73cd006` | `feat(config): strict [engine] limits validated before any lock` |
| 64 | `eacdbda` | `refactor(engine): rewire test imports and fix facade doc link` |
| 65 | `651c719` | `refactor(engine): split engine.rs into production modules` |
| 66 | `df05503` | `refactor(engine): extract inline tests` |

Seven of these cite a review in their own commit message — `7a83e69`, `d6c82fd`,
`ad2ea84`, `f6e7d88`, `1a9cb20`, `bc07139`, `ae9e9da` — and are the better-placed
half. The message is not a verdict record, so they stay residue; a panel that
wants to spot-check should start with `73cd006`, which carries production logic
on the same seam as the `max_parallel` refusal and cites nothing.

**Group 2 — the withdrawn exemption (1 unit, 1 commit).** Named by the PR #80
review.

| Unit | Commit | Why it is not exempt |
|---|---|---|
| 59 | `59a6830` | `docs(process): record the review-effort decision and a standing finding ledger`. Its diff is three files: `decisions/2026-08-17-review-effort-and-fan-out.md`, `reviews/FINDINGS.md` **and `reviews/README.md`**. Only the middle one is exempt. It has no recorded review |

**Group 3 — master-forward merges carrying merge-authored content (3 units, 4
commits).** A "merge master into the branch" unit is only covered when its whole
delta comes from the merged-in parent. These three do not meet that test, and
the difference is conflict resolution — content authored *in the merge*, which no
review on either side ever saw.

| Unit | Commit | Delta files | From merged parent | Merge-authored |
|---|---|---:|---:|---:|
| 35 | `5196c0b` | 24 | 13 | **11** |
| 48 | `4294a16` | 11 | 10 | **1** |
| 60 | `e4b0508` | 1 | 1 | 0 — but its merged parent `cb991b2` is **not an ancestor of `origin/master`**, so the attestation cannot be traced. The history rewrite recorded in `reviews/FINDINGS.md` §14 is why |

The two units that **do** pass the test, and are therefore mapped, are
`615597c` (3 of 3 from `09e79e0`) and `9f1be2b` (13 of 13 from `0cc44d8`).

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
| 35 | `5196c0b` | non-PR | 1 | **R** | Merge master into codex/parallelism-design |
| 36 | `16a8036` | non-PR | 1 | **R** | rename: tactus -> upstroke on the integration branch |
| 37 | `7c4f974` | PR #28 | 7 | **S** | Merge pull request #28 from eventloops/fix/win-sharing-v |
| 38 | `150c2f7` | PR #27 | 28 | **S** | Merge pull request #27 from eventloops/slice/pr6 |
| 39 | `9f1be2b` | non-PR | 1 | **M** | Merge master into codex/parallelism-design |
| 40 | `25a807e` | non-PR | 1 | **X** | docs(reviews): name which provider the exhaustion events |
| 41 | `c3039f4` | non-PR | 1 | **X** | docs(reviews): capacity binds as a rate, not a volume |
| 42 | `302c841` | non-PR | 1 | **X** | docs(reviews): close the catalogue re-measurement at 190 |
| 43 | `5b7417f` | non-PR | 1 | **X** | docs(reviews): file the catalogue re-measurement's findi |
| 44 | `781bff5` | non-PR | 1 | **X** | docs(reviews): give the capacity row its distribution |
| 45 | `9d75632` | non-PR | 2 | **X** | Merge docs/capacity-forward-constraint |
| 46 | `bf15b52` | non-PR | 1 | **X** | docs(reviews): give the carried worktree-residue rows an |
| 47 | `7c4163f` | non-PR | 1 | **X** | docs(reviews): record that a history rewrite orphans the |
| 48 | `4294a16` | non-PR | 1 | **R** | Merge master into codex/parallelism-design |
| 49 | `2746ed6` | non-PR | 1 | **R** | fix(effects): record libc::pipe2 as the Linux-only denia |
| 50 | `7a83e69` | non-PR | 1 | **R** | feat(effects): route every filesystem effect through a f |
| 51 | `20d44a4` | non-PR | 1 | **X** | docs(reviews): record the owner ruling that the frozen f |
| 52 | `d6c82fd` | non-PR | 1 | **R** | fix(runner): pin what the supervision tests assert, and  |
| 53 | `ad2ea84` | non-PR | 1 | **R** | fix(runner): make the CLI ambient-latch test immune to i |
| 54 | `f6e7d88` | non-PR | 1 | **R** | fix(runner): close the three gaps the frontier review of |
| 55 | `31b2b6f` | non-PR | 1 | **X** | docs(reviews): record PR4's CI attestation and the macOS |
| 56 | `510ce2a` | non-PR | 1 | **R** | fix(runner): make three PR4 tests independent of the mac |
| 57 | `1a9cb20` | non-PR | 1 | **R** | feat(runner): CommandSpec, host Runner, typed Invocation |
| 58 | `753790d` | non-PR | 1 | **X** | docs(reviews): close the Windows known-unknown with CI e |
| 59 | `59a6830` | non-PR | 1 | **R** | docs(process): record the review-effort decision and a s |
| 60 | `e4b0508` | non-PR | 2 | **R** | Merge remote-tracking branch 'origin/master' into codex/ |
| 61 | `bc07139` | non-PR | 1 | **R** | feat(topology): schema-4 vocabulary, header-first decode |
| 62 | `ae9e9da` | non-PR | 1 | **R** | feat(topology): TaskKey registry with legacy projection  |
| 63 | `73cd006` | non-PR | 1 | **R** | feat(config): strict [engine] limits validated before an |
| 64 | `eacdbda` | non-PR | 1 | **R** | refactor(engine): rewire test imports and fix facade doc |
| 65 | `651c719` | non-PR | 1 | **R** | refactor(engine): split engine.rs into production module |
| 66 | `df05503` | non-PR | 1 | **R** | refactor(engine): extract inline tests |

## Addendum, 2026-08-31 — PR #80 and the capture unit extend the baseline map

The map above remains the complete mechanical partition of the pre-assembly
baseline `76b6a784..50ed8c86`: **418 commits in 66 first-parent units**. It is
not rewritten to make later evidence look older than it is.

PR #80's integration head `47dc9a35f6e6af59160ece49570d9934a4450dec`
adds one first-parent unit owning seven range commits: the six evidence commits
`50a84acd`, `8dff3e91`, `e174d086`, `ada79bd7`, `2ba66b6e`, and `bc67e7e1`,
plus merge `47dc9a35`. The range through that merge is therefore **425 commits
in 67 first-parent units**. That unit is class **S**, `n = 7`.

The capture commit carrying this addendum advances the promotion head once more.
It is one direct evidence unit, `n = 1`, assigned to class **R** until the fresh
round-two panel reviews it. The final range is consequently **426 commits in 68
first-parent units**:

| Class | Units | Commits |
|---|---:|---:|
| **S** — slice pull request | 34 | 376 |
| **B** — bridge pull request | 2 | 16 |
| **M** — master-forward merge, verified | 2 | 2 |
| **X** — authorised exact-`reviews/FINDINGS.md` exemption | 11 | 12 |
| **R** — residue delegated to the panel | 19 | 20 |

The unit counts sum to **68** and the commit counts sum to **426**. The older
residue set remains 18 units / 19 commits; the one-unit increase is exactly this
capture commit. No historical unit changes class, and no review is claimed in
advance of the fresh panel.
