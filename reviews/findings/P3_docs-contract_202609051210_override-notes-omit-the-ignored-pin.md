---
id: R164-ASTRA-08
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/topology/events.md:2163
provenance: pre_existing
first_bad:
guard: "Compare the description with RungBinding::matches_override and the existing test's for pinned in [true, false] cases."
---

## Failure sequence

A maintainer reads the test description saying override comparison ignores tier and nothing else. RungBinding::matches_override at src/topology/events.rs:684 compares agent, model and effort and ignores both tier and pinned. The following notes paragraph also says the pin is ignored. Treating the first sentence as the test contract could lead to a pin comparison that rejects a valid override.

The reviewer identified a pre-existing Rustdoc claim copied by f45e76a3037657b28686e462106f7d12e670dcfa. The original first bad commit was not established.

This is finding R164-ASTRA-08 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

Correct the first sentence to say that both tier and pin are ignored, keeping it consistent with the implementation and following paragraph. Preserve the existing true/false pin cases.
