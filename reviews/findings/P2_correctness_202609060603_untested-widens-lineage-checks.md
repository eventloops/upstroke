---
id: SWEEP-CHECK_CANDIDATE-003
severity: P2
disposition: deferred
category: correctness
pr: 186
reviewed_sha: f301b7b38e60c0df00ba89c0a42fa179bf017c01
location: src/topology/fold/check_candidate.rs:114
provenance: pre_existing
first_bad:
guard: needs a WidensLineage case in src/topology/fold/tests.rs (row 39) with a mismatched root
  and a mismatched region
---

## Failure sequence

`check_candidate_prepared`'s `WidensLineage` arm refuses when the record's `root` disagrees with
the task's own `entry.lineage.root` (line 115), and separately when the record's `paths` disagree
with `prepared.actual_paths` (line 124). Either check is edited or removed -> a candidate record
claiming to widen the wrong lineage, or to widen by a region its diff did not actually touch, is
accepted as a valid `candidate_prepared` settlement -> the fold's lease table (via
`apply_candidate_prepared`'s `self.leases.widen_lineage(*root, paths)`) is then widened by an
attacker- or defect-supplied `root`/`paths` pair the record's own claimed lineage never
authorized. Two independent mutations (M9: disable the root check; M10: disable the region check,
both in the pull request body) each survive not just this file's owning test module but the whole
crate's test suite (2,134 passed, 0 failed) with either check disabled.

## What the change that takes this up should do

No live defect: both checks are present and correct in the code today, read against
`docs/internals/topology/fold/check_candidate.md`'s account of INV-09-style lineage widening. The
gap is purely regression coverage. Row 39 (`src/topology/fold/tests.rs`) needs two cases alongside
the existing `widening` test (which only exercises the *pairing* refusal at
`check_candidate.rs:132-134`, not these two *value* checks): a `candidate_prepared` for a genuine
lineage member whose `WidensLineage.root` names a different task than its own `entry.lineage.root`,
and one whose `WidensLineage.paths` disagrees with `actual_paths`, each asserting
`FoldError::InconsistentRecord` and each witnessed failing against M9/M10 respectively before the
fix and passing after (this pull request's own repro: apply M9 or M10 from the pull request body
and re-run `cargo test --all-targets --all-features` to see it survive).
