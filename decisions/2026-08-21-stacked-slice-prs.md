# 2026-08-21 — slices land as pull requests into their integration branch

**Verdict.** From the next slice (PR6) onward, each slice of the parallel-execution design is
a branch off `codex/parallelism-design` and a pull request **into** it — not a run of commits
on it. Slice pull requests receive `upstroke-ci`, `upstroke-pr-policy`, and a single-reviewer
frontier review of each head; they are **not attested**. The App-owned `upstroke-frontier-review`
check is minted only for pull requests into `master`, so the integration branch's own pull
request (#18) is attested exactly once, on the head that merges, after its last update from
`master`. Slice merges use merge commits; the integration branch is never rebased or squashed.

## Why

- **Reviewers should see the slice, not the pile.** A review of #18 today diffs
  `merge-base(master)..head` — every slice since PR0b, sixteen commits and climbing toward
  fourteen slices. A slice pull request's diff is the slice. This is the same argument the
  review-gate record makes for reviewing the head you intend to merge: judge the artifact, not
  its history.
- **It is the schedule the gate record already decided.** [2026-08-20 — the automated review
  gate](2026-08-20-automated-review-gate.md) rules: single reviewer on every head before the
  merge candidate, panel once on the merge candidate. Slice pull requests are the rounds before;
  #18's final head is the candidate. Nothing new is being invented — the structure is being
  made visible on GitHub.
- **Per-slice ledgers are already the convention.** `reviews/FINDINGS.md`'s own rule: "per-PR
  ledgers in pull-request bodies stay as they are … this file is their union." One
  fourteen-slice ledger in #18's body is the alternative, and it is the worse one.
- **It matches how the work is already done.** The `pr3-*` and `pr4-*` worktrees on the build
  box are per-slice branches in everything but name.

## What is gated where

| Pull request | `upstroke-ci` | `upstroke-pr-policy` | frontier review | App attestation |
|---|---|---|---|---|
| slice → `codex/parallelism-design` | yes | yes | single reviewer per head, report only | **no** |
| `codex/parallelism-design` → `master` (#18) | yes | yes | panel, once, on the merge candidate | yes, exactly once |

The attestation workflow's refusal of non-default base branches is **unchanged and
deliberate**. This record changes workflow *triggers*; it moves no trust boundary.

## The one thing that breaks the model, stated now

**Rewriting the integration branch orphans ledger rows.** `validate-pr-ledger-evidence.sh`
requires each row's reviewed SHA to be an ancestor of the exact head; a rebase, squash, or
force-push that replaces commits makes every row citing a replaced SHA fail at attestation.
Measured on 2026-08-21, the day this record was written: a co-author rewrite of
`codex/parallelism-design` was in flight, and every #18 ledger row bound to a pre-rewrite SHA
will need re-binding to its successor before Gate2/PR7. Hence merge commits only, on the
integration branch and into it.

## Options rejected

- **Keep accumulating commits on the integration branch.** Zero setup, and the reason this
  record exists: the review unit degrades with every slice, and the body ledger becomes a
  single ever-growing table.
- **Slice pull requests straight into `master`.** Merges an unfinished design into the default
  branch slice by slice, and every merge puts every other open pull request behind `master`
  under the strict up-to-date rule — the cascade the gate record warns about.
- **Attest slice pull requests too.** Would require the attestation workflow to accept
  non-default bases — a trust-boundary change with its own record, and unnecessary: the
  integration branch's single attestation already covers what merges.

## Measured vs assumed

Measured: `ci.yml` and `pr-policy.yml` triggered only on `branches: [master]` before this
change (checked 2026-08-21); #18 at sixteen commits ahead of `master`; the ancestry check in
`validate-pr-ledger-evidence.sh`. Assumed, and named: that a second ruleset on the integration
branch (requiring the two checks and merge commits, no App check) is wanted — that is an
owner-side settings change this record recommends but cannot make.

## Cross-references

- [2026-08-20 — the automated review gate](2026-08-20-automated-review-gate.md) — the
  schedule this record applies.
- [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md)
  — the ancestry-based rules that make rewriting the integration branch costly.
- MAINTAINING.md — the paragraph after step 8, added with this record.
