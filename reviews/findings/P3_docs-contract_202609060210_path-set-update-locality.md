---
id: PR167-PATH-SET-UPDATE-LOCALITY
severity: P3
disposition: deferred
category: docs-contract
pr: 167
reviewed_sha: 75e262a04e39388d5bc1bb623284fc666edac508
location: docs/internals/validate/graph.md:81
provenance: introduced_by_feature
first_bad:
guard: follow-up standards and graph-notes reconciliation
---

## Failure sequence

Read the note claiming one push and one pop -> inspect root initialization and dependency descent -> find two independent push/update sites. The invariant holds; the claimed locality is false.

[Independent review](https://github.com/eventloops/upstroke/pull/167#issuecomment-5556267517), gpt-5.6-sol at max, returned CHANGES_REQUIRED. No graph-runtime counterexample was found. Passing CI does not resolve this finding.

## What the change that takes this up should do

Correct the note to name both root initialization and dependency descent and the shared pop operation. No traversal defect was demonstrated.
