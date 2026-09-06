---
id: SWEEP-RESIDUE-AUTHORITY-002
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/sites.rs:1183
provenance: pre_existing
first_bad: —
guard: the `src/topology/effects/sites.rs` sweep (queue row 25), with `src/topology/effects/vocab.rs` (row 26) if the answer is a new row
---

## Failure sequence

`rundir`'s `cleanup::take` opens `<public>/cleanup.lock` with `create(true)`, so
`Lock.ProbeCleanupExclusive` brings a durable file into existence -> that site's `row()` is
R17, "the coordinator's own lock holds (OS lock state only)", which is a `ProcessLocalOs`
row and cannot account for a file -> the site's after-phase entry names no row for the file
it created, and a reader of the entry cannot see that anything durable was left

The lock funnel makes the distinction everywhere else: the repository-scoped lock has two
sites and two rows, `Lock.CreateWorktreeLockFile` for the file (R25, external-physical) and
`Lock.AcquireWorktree` for the hold (R17). The cleanup lock has one site and one row, and
the file falls between them. R21 ("run-scoped run-directory contents, public and private")
does account for it in the ledger, but no site names R21 here.

## What the change that takes this up should do

Decide which of the two shapes the cleanup lock takes: a second site
(`Lock.CreateCleanupLockFile`) with the row for the file, mirroring the worktree lock, or
`ProbeCleanupExclusive` answering R21 for the file it publishes. Either changes `sites.rs`'s
`row()` or its variant list, and the second may need a row in `vocab.rs`; neither is inside
the `residue_authority.rs` sweep's bound.

`residue_authority.rs` was swept on branch `sweep/topology-effects-residue-authority` and
now answers `AfterEffect::MomentaryHold` for this site — "no row is left holding it" — which
is true of the *hold* the probe releases and says nothing about the file it creates. That
repair is correct and this is the residue it leaves.
