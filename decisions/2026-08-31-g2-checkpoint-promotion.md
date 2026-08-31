# 2026-08-31 — the G2 checkpoint promotion candidate, and what it discharges

**Verdict.** This promotion is **the G2 checkpoint** that
`2026-08-25-checkpoint-merges.md` ordered. That record stays **controlling in
full**: nothing in it is superseded, narrowed, or set aside here. This record is
the reconciliation — it names each obligation that record imposes, points at the
evidence that discharges it, and states plainly which obligations are discharged
**now** and which are discharged **only when the panel and the serialized suite
report**. A candidate is assembled; a candidate is not a pass.

The candidate is `promotion/g2-candidate-assembly`, cut at
`50ed8c86ec60164011bfd393066c4c3696d3865b`. `master` before the promotion is
`76b6a784ae5562ac044d6ff9a15b68397bd9b0e0`.

## What the checkpoint record obliges, and where each obligation stands

| # | Obligation (`2026-08-25-checkpoint-merges.md`) | Where it is discharged | State at this candidate |
|---|---|---|---|
| 1 | #18 becomes the G2 merge; the candidate is cut after the G2 gate passes on the cleaned tree | `reviews/2026-08-31-g2-gate-report.md` | **Partly.** The candidate is cut and the gate report exists as artifact 1. The gate **has not passed**: six of eight artifacts are owed to execution this host may not perform. See §"What is not discharged" |
| 2 | Every open `reviews/FINDINGS.md` row triaged to repaired / carried-with-owner-and-re-opening-condition / closed-not-owed, before the candidate is cut | `reviews/FINDINGS.md` §35 | **Discharged.** 65 logical §2 rows normalized latest-disposition-wins; every live row lands in exactly one of the three buckets |
| 3 | §4's recurrence classes reviewed for structural guards at the same sitting | `reviews/FINDINGS.md` §35, "Recurrence classes" | **Discharged.** All 18 classes reviewed; each carries a guard verdict of mechanical, partial, or convention-only |
| 4 | The three-model panel attests the code **and** the audited ledger together | not in this repository | **Owed.** This commit is assembly, not attestation. No review of any kind was run to produce it |
| 5 | The G2 gate's eight artifacts produced on the extended input range before the candidate is cut; the gate report is artifact 1 | `reviews/2026-08-31-g2-gate-report.md` | **Partly.** Artifact 1 exists. Artifact 7's inputs are present and hash-pinned. Artifacts 2–6 and 8 are named, scoped, and **owed** |
| 6 | Inert by default, **verified not assumed**; the panel confirms it on the candidate | `reviews/2026-08-31-g2-gate-report.md` §"Inert by default" | **Discharged structurally, owed behaviourally.** Five proofs are verified by construction at this base; the panel's confirmation is still the panel's to give |
| 7 | No `0.2.0` tag until v0.2 completes | this record, and the gate report | **Discharged.** This promotion authorizes no tag. `Cargo.toml` stays `0.1.0`; `v0.1.0` is still the only tag |

## The input range, and the authority for it

The G2 gate's own `frozen_input_range` field names the unextended range —
PR4, PR5, PR6 and PR7 merged on top of G1. `2026-08-25-checkpoint-merges.md`
requires the artifacts on the **extended** range instead, per packet erratum
**E5** (`upstroke-lab:packet/2026-08-25-g2-pass-errata.md`), which adds the G2
pass slice and the whole-tree sweep slice.

The extended range is what this candidate is assembled against. The authority is
the owner's direct promotion amendment of 2026-08-31, which requires the eight
artifacts over the E5-extended range; that amendment is the adoption. The
erratum itself lives in the private companion repository and is not reproduced
here.

**One thing a later reader should not have to re-derive:** the packet record's
own range field and the range this candidate is measured against are different,
deliberately, and the difference is E5 plus the owner's amendment — not drift.

## What is not discharged, stated so it cannot be mistaken for a pass

- **The gate has not passed.** Six of the eight artifacts require executing the
  full suite, the Docker-gated suite, and the hosted macOS and Windows legs.
  Assembly does not produce them and this record does not claim them.
- **No attestation.** Obligation 4 is untouched. The three-model panel has not
  run, and an agent session has no standing to merge to `master` in any case
  (`CLAUDE.md`; `2026-08-23-retire-app-attestation.md`).
- **The full suite has not run for this commit.** Root serializes it globally
  after the commit lands. Nothing here asserts a suite result.

## Rollback, and why its bar is deliberately high

The eventual merge of this candidate into protected `master` is rolled back with

```
git revert -m 1 <MERGE_OID>
```

`-m 1` keeps `master`'s own first-parent line and undoes the second parent — the
whole integration side — in one commit. That is the only rollback this promotion
has, and it is a one-way door in practice: once the revert is on `master`,
**re-promoting the same work requires reverting the revert**, because the
original merge base is already an ancestor and a plain re-merge brings nothing
back. A second promotion therefore starts from a revert-of-a-revert, which is
harder to review than the original merge and reads worse in history.

That bar is intended. The checkpoint exists to move the tree at its
maximum-confidence moment; a rollback that were cheap would invite merging
before that moment arrived.

## Relationship to other records

- `2026-08-25-checkpoint-merges.md` — **controlling, unchanged.** This record
  reconciles against it and supersedes no part of it.
- `2026-08-21-stacked-slice-prs.md` — unchanged; already superseded in part by
  the checkpoint record, not further by this one.
- `2026-08-20-review-invalidation-scope.md` — unchanged, and it governs this
  candidate: a push whose whole diff from a reviewed head is confined to
  `reviews/FINDINGS.md` keeps the review, and everything else invalidates it.
  This commit touches `decisions/` and `reviews/` beyond that file, so it is
  **not** exempt-only and does not inherit any earlier head's review.
- `2026-08-23-retire-app-attestation.md` — unchanged; the owner's merge remains
  the attestation.
