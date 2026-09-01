# 2026-09-01 — review effort is re-scoped: serious P1s block, the rest is fixed or logged as baggage

**Verdict.** One full frontier review pass per pull request remains the norm.
Three classes of finding block a merge. A serious P1 (`MAINTAINING.md`,
"Finding triage and cleanup points") is fixed and obliges a re-review of the
repaired head. Two standing rules outrank the bar and are preserved, not
narrowed: a `MUST` deviation in materially touched code is never baggage,
and a finding carrying a failing test, reproduction, or mutation witness
blocks whatever its label; both are fixed before merge, their repair delta
verified like any other. Every other finding is fixed at the author's
discretion or logged with disposition `accepted-risk` or `deferred`; the
merge then proceeds on the completed pass plus the owner's verification of
the repair delta, disclosed in the pull-request body. A completed pass
containing no serious P1 is the pull request's full pass whatever its
verdict line says, and is recorded as written; a `CHANGES_REQUIRED` whose
findings all landed as repairs or logged baggage is never recorded as a
pass. The lane is for single-reviewer passes: a panel-reviewed merge
candidate re-runs every seat on any head movement. Accepted baggage is swept
at designated cleanup points: before any release, at integration checkpoint
merges, and at owner-called sweeps.

## Why

Measured cost of iterate-until-clean on documentation-weight changes: nine
review passes on #83; five non-converging passes on the stopped #25 gate
experiment; one completed pass plus a deliberately stopped second on #87, a
two-file documentation change. Every pushed head paid a full three-OS CI
matrix (roughly sixteen minutes, the Windows leg dominant) and every pass a
frontier review of ten to forty minutes; #83's ninth pass alone re-reviewed
a body-only repair on an already-green head, with no new matrix. The loop
was built to protect engine code and was
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
- `2026-08-20-automated-review-gate.md`'s scheduling rule — a single
  reviewer on every head — narrows the same way; its §3 adjudication
  routing (a failing test, reproduction, or mutation witness blocks,
  whatever the severity), its §5 conditions on any automated return (no
  automated process merges, reviewers hold no attesting credential), and
  its panel cadence are untouched. A dated forward notice is appended
  there in the same change.
- `2026-08-21-stacked-slice-prs.md`'s per-head review of slice pull
  requests narrows identically; gates, the merge-commit rule, and the
  attestation cadence are unchanged. A dated forward notice is appended
  there in the same change.
- `2026-08-23-retire-app-attestation.md` kept the obligation unchanged when
  the App check retired: every head that merges reviewed first. That
  obligation narrows to the lane above; the owner's merge stays the
  attestation, now of the delta accounting too, and §5's bar for any
  automated return is untouched. A dated forward notice is appended there
  in the same change.
- `2026-08-25-checkpoint-merges.md` restates the per-head slice ceremony as
  unchanged by that record; the ceremony's narrowing reaches it through the
  2026-08-21 notice, and its checkpoint merge doubles as a cleanup point. A
  dated forward notice is appended there in the same change.
- `2026-08-31-panel-seats.md` is untouched: a checkpoint candidate is
  panel-reviewed on the head that merges, and any head movement after a
  seat has run re-runs every seat. The owner-verified repair lane applies
  to single-reviewer passes only.
- The ledger schema and its validators are untouched: `accepted-risk` and
  `deferred` were already canonical dispositions.

## Precedent

#87 merged on 2026-09-01 under an explicit owner waiver disclosed in its
body: its one completed pass required changes, both findings were repaired,
and its second pass was deliberately stopped. No lane existed for that; the
waiver was ad hoc. This record is the durable rule that replaces such
waivers: a completed pass with findings, their triage, and an
owner-verified repair delta. #87 is its precedent, not an exercise of it,
and stays recorded as its own body states.

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
- [2026-08-23 — retire the App-signed attestation gate](2026-08-23-retire-app-attestation.md)
  — the obligation it kept unchanged, narrowed here by notice.
- [2026-08-25 — integration merges happen at attested checkpoints](2026-08-25-checkpoint-merges.md)
  — the checkpoint cadence that doubles as a cleanup point.
- [2026-08-31 — the G2 panel's three seats](2026-08-31-panel-seats.md)
  — the every-seat re-run this record leaves outside the narrowing.

## 2026-09-01 — the exempt set this record left unchanged is widened

The first bullet under "What this narrows" says the exempt path set is
exactly `reviews/FINDINGS.md`. Later the same day
`2026-09-01-clean-base-merge-keeps-review.md` widened it by successor
record: a conflict-free merge of the base that leaves the pull request's
diff byte-identical, with CI green on the merged head, keeps the review.
The lane this record defines is untouched; a clean merge-in is exempt, not
owner-verified. See
[2026-09-01 — a clean merge of the base keeps the review](2026-09-01-clean-base-merge-keeps-review.md).
