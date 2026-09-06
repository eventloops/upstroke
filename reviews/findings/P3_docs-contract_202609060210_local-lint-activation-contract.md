---
id: PR167-LOCAL-LINT-ACTIVATION-CONTRACT
severity: P3
disposition: deferred
category: docs-contract
pr: 167
reviewed_sha: 75e262a04e39388d5bc1bb623284fc666edac508
location: src/validate/graph.rs:4
provenance: introduced_by_feature
first_bad:
guard: follow-up standards and graph-notes reconciliation
---

## Failure sequence

Enable indexing_slicing and unreachable locally -> a graph test using indexing is linted differently from the canonical Cargo.toml policy -> the stated staged lint-activation contract is contradicted. The reviewer identified this by source and standards inspection, not a newly executed failing test.

[Independent review](https://github.com/eventloops/upstroke/pull/167#issuecomment-5556267517), gpt-5.6-sol at max, returned CHANGES_REQUIRED. No graph-runtime counterexample was found. Passing CI does not resolve this finding.

## What the change that takes this up should do

Reconcile local lint activation with standards/02 and standards/SWEEP.md while preserving the intended panic policy. Update the graph notes, sweep row and historical guard claims to match the chosen policy.
