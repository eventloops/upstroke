---
id: PR5-R2-SNAPSHOT-INPUT-COMMIT-DEAD
severity: P3
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer (the slice that first requests two snapshots)
---

## Failure sequence

`PR5-WORKSPACE-024` and `PR5-WORKSPACE-025`. `SnapshotInput::Commit` is constructed **nowhere** in the tree, so `create_integration_snapshot`'s "check out the proposal or head commit and create no object" arm never executes and turning it into an unconditional commit-tree synthesis changes nothing any test runs; and `add_snapshot` has two callers in two different tests with two different fixtures, so no fixture ever holds a gate snapshot and a reviewer snapshot alive **together** and `ExactSnapshotStore::create` caching one snapshot for every role and attempt is invisible. `SnapshotName::review` is constructed nowhere either

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer (the slice that first requests two snapshots).

**Carried: the caller does not exist yet, and inventing one inside a repair round is inventing the orchestration.** Both entries need a *second live request* — an integration snapshot from a proposal commit, and a gate snapshot plus a reviewer snapshot alive at once across two attempts — which is the gate/review orchestration PR5's `scope` stops before. The live passages are `workspace_candidates.snapshots`: "integration snapshots check out the proposal or head commit and create no object" and "one snapshot for the gate set and one fresh snapshot per reviewer, never reused across roles or attempts". Recorded rather than dropped so the first slice that builds a reviewer snapshot knows it inherits an unmeasured claim

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
