# 2026-09-01 — a clean merge of the base into a reviewed head keeps the review

**Verdict.** A merge commit that brings the pull request's base into a
reviewed head keeps the recorded review when all four hold: git reported no
conflict; the pull request's diff against its base is byte-identical before
and after (`git diff <base>...<head> | sha256sum`, both hashes recorded in
the body); CI is green on the merged head; and the pull request itself
edits no workflow, gate script, or validator. The body records the reviewed
SHA, the merged head, the old and new base SHAs, and both diff hashes. A
merge-in that fails any condition is a new head under
`2026-09-01-review-effort-rescoped.md`. Panel-reviewed merge candidates are
outside this record: `2026-08-21-stacked-slice-prs.md` reviews them on the
head that merges, after the last update from `master`, and
`2026-08-31-panel-seats.md` re-runs every seat on any head movement.

## Why

The `master` ruleset requires an up-to-date branch, so every merge to
`master` forces every other open pull request to merge `master` in: a new
head, a sixteen-minute matrix, and under the invalidation rule a review
that no longer binds. The review read a diff. When the merge-in leaves that
diff byte-identical, a fresh pass re-reads the same text against a base
whose own commits were each reviewed in their own pull requests. With the
W1 packet groups and the W2 funnels of the decomposition landing serially,
that is one pass per merge for zero change in what was reviewed. The owner
ruled the trade on 2026-09-01: a conflict-free merge-in is exempt.

## What this widens, stated so nothing is silently overturned

- `2026-08-20-review-invalidation-scope.md` says widening the exempt set is
  a successor decision record. This is that record. The exempt set is now
  pushes confined to `reviews/FINDINGS.md`, and clean base merge-ins as
  defined above. The binding rule stands: the merged head is a different
  tree, and the body says so. A dated forward notice is appended there in
  the same change.
- `2026-09-01-review-effort-rescoped.md` is unchanged in substance: its
  owner-verified lane is for repair deltas, and a clean merge-in is exempt,
  not verified. Its statement that the exempt set is exactly
  `reviews/FINDINGS.md` is dated by a forward notice appended in the same
  change.
- The accepted risk, named: a semantic interaction between the base's new
  commits and the unchanged diff (a renamed callee, a moved invariant) is
  invisible to a diff hash. CI green on the merged head is the guard the
  project accepts for it; a failure there is a new head, reviewed.

## Rejected

- **Dropping the up-to-date requirement.** The merge-in is what makes CI on
  the merged tree meaningful; an out-of-date merge tests a tree nobody
  built. MAINTAINING's ruleset clause stays.
- **Re-reviewing on every merge-in.** The cost above, for a diff that has
  not changed.
- **Exempting any merge-in, conflicts included.** A conflict resolution is
  authored content nobody reviewed.
- **Exempting a merge-in that changes the diff.** One drifted hunk means the
  reviewed text and the merged text differ; the hash rule fails closed.

## Cross-references

- [2026-08-20 — what invalidates a frontier review](2026-08-20-review-invalidation-scope.md)
  — the exempt set this record widens.
- [2026-09-01 — review effort is re-scoped](2026-09-01-review-effort-rescoped.md)
  — the repair lane this record leaves untouched.
- [2026-08-21 — slices land as pull requests into their integration branch](2026-08-21-stacked-slice-prs.md)
  — the merge-candidate rule that keeps panel candidates outside.
- [2026-08-31 — the G2 panel's three seats](2026-08-31-panel-seats.md)
  — every seat re-runs on any head movement, untouched.
