---
id: PR179-DOCS-DEAD-DECISIONS-PATH
severity: P4
disposition: deferred     # out of this sweep's file scope; needs its own pass across docs/internals
category: docs-contract
pr: 179
reviewed_sha: 655771a3c1353520dd21fac143abcb6dbc6be252
location: docs/internals/topology/registry.md:275; docs/internals/topology/fold/tests.md:358,1607; docs/internals/runner/host/environment.md:120; docs/internals/engine/topology/run.md:1257; docs/internals/engine/topology/candidate.md:68; docs/internals/engine/topology/recover/tests.md:1828,1861; docs/internals/engine/topology/attempt.md:65; docs/internals/engine/topology/settle.md:258; docs/internals/engine/topology/settle/tests.md:225; docs/internals/engine/topology/candidate/tests.md:482
provenance: pre_existing
first_bad: 2026-09-03 (the decisions/, proposals/ and acceptance/ directories were retired)
guard: whichever sweep or docs pass next touches one of these files
---

## Failure sequence

`decisions/` was retired repository-wide on 2026-09-03 (`CLAUDE.md`: "the decisions, proposals and
acceptance directories were retired ... the `DESIGN.md` index says where each record's substance
now lives"). `decisions/2026-08-12-merge-queue-execution-topology.md` no longer exists anywhere in
the tree; its substance is recorded, per `DESIGN.md`'s decision-history table (line 87), in
`design/26_design_merge_queue_protocol.md` (§26) — confirmed by grep: the "sole successful
settlement" text this task's own file quoted verbatim is there, at line 156. The eleven citations
above (found by `grep -rln 'decisions/2026-08-12-merge-queue-execution-topology'
docs/internals/`, minus this task's own `docs/internals/topology/fold/check_attempt.md`, fixed in
this PR) still point at the dead path. A reader who follows any of them gets a "no such file"
instead of the design section that actually carries the ruling.

## What the change that takes this up should do

Repoint each citation at `design/26_design_merge_queue_protocol.md` (§26), the way this PR's own
fix does for `check_attempt.md`, and spot-check whether any of the eleven quotes verbatim text
that has since been reworded when it moved into DESIGN.md (this PR only verified the one quote its
own file uses). None of these eleven files are in this sweep's assigned scope
(`src/topology/fold/check_attempt.rs` and its paired notes only), so no code or notes file outside
that pair is touched here.
