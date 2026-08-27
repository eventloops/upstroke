# 2026-08-25 — integration merges happen at attested checkpoints

**Verdict.** `codex/parallelism-design` merges to `master` at **attested
checkpoints** rather than once at v0.2 completion. The first checkpoint is
**G2**: pull request #18 becomes the G2 merge, its candidate cut after the G2
gate passes on the cleaned tree (packet erratum E5,
`upstroke-lab:packet/2026-08-25-g2-pass-errata.md`). A second integration
merge lands at v0.2 completion. Between checkpoints, slice PRs continue into
the integration branch under the existing lighter per-head ceremony — moving
them to master would put full attestation on every slice, the overhead the
stacked-slice decision exists to avoid.

**Every checkpoint candidate carries a full-ledger audit.** Before the
candidate is cut, every open row in `reviews/FINDINGS.md` is triaged to
exactly one of: **repaired**; **carried**, re-dated, with a named venue and a
re-opening condition (the `shrinks_when` pattern); or **closed as not owed**,
with the reason. §4's recurrence classes are reviewed for structural guards
at the same sitting. The three-model panel then attests the code **and** the
audited ledger together: nothing reaches master untriaged.

This **supersedes in part** `2026-08-21-stacked-slice-prs.md` ("attestation
stays master-only and happens once on #18's merge candidate"): attestation
remains master-only and per-checkpoint; "once" becomes "once per checkpoint".
The per-head slice ceremony, the review-invalidation scope
(`2026-08-20-review-invalidation-scope.md`), and the owner's-merge-is-the-
attestation rule (`2026-08-23-retire-app-attestation.md`) are unchanged.

## Why

- **The checkpoint captures the tree at its maximum-confidence moment.**
  Post-pass, post-sweep, post-gate is the most-verified state the codebase
  reaches; holding the merge until v0.2 completion means that attested state
  never lands on master as a state, only as history under later slices.
- **Two smaller attested merges beat one giant one.** Review quality
  degrades superlinearly with diff size; the v0.2-complete diff would be
  nearly double the G2 diff.
- **The split-brain is a measured cost.** In one week the master/branch
  divergence produced: `reviews/FINDINGS.md` and `clippy.toml` invisible
  from master, a three-vs-four gates discrepancy, two `CLAUDE.md` realities,
  and one recorded review error from verifying in the wrong tree
  (`reviews/2026-08-25-pr32-coding-standards-review.md`, re-review section).
  Merging rejoins the trees and puts the full suite under master's required
  CI contexts for the first time.
- **The sweep's result reaches the public trunk immediately**, instead of
  master carrying the normative standard while its own code sits unswept for
  the remainder of v0.2.

## Conditions

- **Inert by default, verified not assumed:** the legacy v0.1 path unchanged
  (the continuously enforced invariant), schema-4 machinery engaged only by
  explicit schema choice; the panel confirms this on the candidate.
- **No `0.2.0` tag** until v0.2 completes; versioning stays honest the way
  capacity-read-only already established.
- The G2 gate's eight artifacts (`cumulative_review_gates.gates[G2]`) are
  produced on the extended input range per E5 before the candidate is cut;
  the gate report is artifact #1 of the candidate's evidence.

## Rejected

- **Hold #18 until v0.2 completes** (the original plan). Rejected for the
  four costs above; the one argument for it — master never carries
  unfinished machinery — was already crossed deliberately and successfully
  by shipping the capacity engine read-only in v0.1. Inert-plus-attested on
  the trunk beats complete-but-divergent on a side branch.
- **Checkpoint merge without the ledger audit.** Rejected: it would move
  untriaged debt onto master, violating the rule the audit generalises —
  a measurement taken and not triaged is worse than one never taken.
