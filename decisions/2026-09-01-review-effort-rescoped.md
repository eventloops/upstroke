# 2026-09-01 — review effort is re-scoped: serious P1s block, the rest is logged baggage

**Verdict.** One full frontier review pass per pull request remains the norm.
Only findings meeting the serious-P1 bar (`MAINTAINING.md`, "Finding triage
and cleanup points") block a merge and oblige a re-review of the repaired
head. Every other finding is fixed at the author's discretion or logged with
disposition `accepted-risk` or `deferred`; the merge then proceeds on the
stale pass plus the owner's verification of the repair delta, disclosed in
the pull-request body. Accepted baggage is swept at designated cleanup
points: before any release, at integration checkpoint merges, and at
owner-called sweeps.

## Why

Measured cost of iterate-until-clean on documentation-weight changes: nine
review passes on #83; five non-converging passes on the stopped #25 gate
experiment; two passes plus a deliberately stopped third on #87, a two-file
documentation change — each iteration paying a full three-OS CI matrix
(roughly sixteen minutes, the Windows leg dominant) and a frontier review
of ten to forty minutes. The loop was built to protect engine code and was
pricing documentation and engine changes identically. The owner ruled the
trade on 2026-09-01: development speed is part of what the process must
protect.

## What this narrows, stated so nothing is silently overturned

- `2026-08-20-review-invalidation-scope.md` stays correct about **binding**:
  a push still means the recorded pass reviewed a different head, and no
  pull-request body may claim otherwise. What changes is the
  **consequence**: a stale pass plus an owner-verified repair delta
  containing no serious P1 is now mergeable, disclosed. The exempt path set
  (exactly `reviews/FINDINGS.md`) is unchanged. A dated forward notice is
  appended to that record in the same change.
- `2026-08-20-automated-review-gate.md` §5 is untouched: no automated
  process merges, and reviewers hold no attesting credential.
- The ledger schema and its validators are untouched: `accepted-risk` and
  `deferred` were already canonical dispositions.

## First exercise

#87 merged on 2026-09-01 with its second review deliberately stopped by the
owner and the waiver disclosed in its body, under the owner's same-day
direction. This record is the durable form of that direction.

## Rejected

- **Keeping iterate-until-clean.** The measured cost above; past the first
  pass, findings on documentation-weight changes were overwhelmingly
  wording-level.
- **Abolishing the frontier review.** A solo-writer trust model has exactly
  one independent check; removing it removes the trust wedge the product
  claims.
- **Path-based tiering (documentation against engine).** #87's own second
  finding showed a documentation change carrying a live correctness
  mitigation. The bar is severity of consequence, not path.

## Cross-references

- [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md)
  — the binding rule this record leaves intact and the consequence it
  narrows.
- [2026-08-25 — integration merges happen at attested checkpoints](2026-08-25-checkpoint-merges.md)
  — the checkpoint cadence that doubles as a cleanup point.
