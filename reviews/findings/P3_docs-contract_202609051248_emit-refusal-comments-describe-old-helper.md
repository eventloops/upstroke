---
id: PR149-EMIT-REFUSAL-COMMENTS-DESCRIBE-OLD-FUNNEL
severity: P3
disposition: deferred
category: docs-contract
pr: 149
reviewed_sha: 7bd886353075d131d1211db1fba0bc77c4f2ef57
location: src/rundir/scratch_tree.rs:519
provenance: fix_regression
first_bad:
guard: source inspection of refuse_to_reclaim and its emit fixture caller; correct the caller comments when updating this fixture
---

## Failure sequence

The changed helper is at src/rundir/scratch_tree.rs:519. The stale caller
comments are at src/engine/topology/emit/tests.rs:176 and
src/engine/topology/emit/tests.rs:165.

A maintainer reads the emit refusal comment and attributes its passing
cleanup-failure witnesses to the real reclaim funnel and its NotFound
classification. The helper now constructs PermissionDenied directly with the
unchanged token. It performs no identity check, removal or absence observation.
The tests still cover fixture refusal handling, but the comment overstates
which allocator path they exercise. The adjacent reclaim summary also omits
the new independent root-absence observation.

This is a nonblocking documentation defect. No runtime regression, failed gate
or applicable MUST violation is claimed. The helper's own updated contract is
accurate. The first-bad commit was not separately established.

The owner authorized deferral of this limited documentation finding under
STACK_STOP_RULE.md. The independent review returned PASS with this record and
its canonical ledger row required before handoff. The source comments remain
unchanged in the final record commit.

## What the change that takes this up should do

Describe the synthetic refusal in the emit caller and state that actual reclaim
success depends on observed root absence. Keep the real allocator witnesses
separate from fixture reporting witnesses.
