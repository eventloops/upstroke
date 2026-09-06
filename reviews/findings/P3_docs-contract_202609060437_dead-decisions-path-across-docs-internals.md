---
id: PR180-DOCS-DEAD-DECISIONS-PATH
severity: P3
disposition: deferred     # out of this sweep's file scope; needs its own pass across docs/internals
category: docs-contract
pr: 180
reviewed_sha: 50bcfab07c89488e354a7183ecbddcf35f18c2cc
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
settlement" text this task's own file quoted verbatim is there, at line 156. The **twelve citation
sites across the ten files** listed above still point at the dead path (two of those files,
`docs/internals/topology/fold/tests.md` and `docs/internals/engine/topology/recover/tests.md`,
carry two sites each; the other eight carry one). A reader who follows any of them gets a "no such
file" instead of the design section that actually carries the ruling.

The census is `decisions/2026-08-12-merge-queue-execution-topology` counted over
`docs/internals/**/*.md`, minus this task's own `docs/internals/topology/fold/check_attempt.md`,
which this PR fixes. It was re-derived with two independent engines (ripgrep and a Python
occurrence count) after review pass 1 found the first version of this sentence wrong: the row and
this file had said "nine files", and pass 1's own correction said "eleven citation sites". Both
numbers were short. Ten files, twelve sites, on both engines.

## What the change that takes this up should do

Repoint each of the twelve citations at `design/26_design_merge_queue_protocol.md` (§26), the way
this PR's own fix does for `check_attempt.md`, and spot-check whether any of them quotes verbatim
text that was reworded when it moved into DESIGN.md (this PR only verified the one quote its own
file uses). Re-derive the census before restating it rather than copying the numbers above; they
have already been wrong twice. None of these ten files are in this sweep's assigned scope
(`src/topology/fold/check_attempt.rs` and its paired notes only), so no code or notes file outside
that pair is touched here.

Where the retired label `INV-07` accompanies one of these citations, the same treatment this PR
applied to `check_attempt.md` fits: §26 is the authority, and `INV-07` is the retired packet's
label for the rule, not a second authority — no living design section defines any `INV-` id.
