---
id: R164-ASTRA-07
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/topology/schema.md:198
provenance: pre_existing
first_bad:
guard: "Compare the documented error partitions with select_for_schema and its crossed schema/ceiling tests, including schema 3 with ceiling 2."
---

## Failure sequence

A caller reads the Errors section as limiting NewerThanReadable to schemas above topology schema 4. select_for_schema at src/topology/schema.rs:120 instead refuses every non-topology schema above the supplied ceiling. Schema 3 with ceiling 2 returns NewerThanReadable even though 3 is below 4. The documented partition therefore omits a real refusal for a legacy log.

The reviewer identified a pre-existing Rustdoc claim copied into the notes by f45e76a3037657b28686e462106f7d12e670dcfa. The original first bad commit was not established.

This is finding R164-ASTRA-07 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

Describe the comparison with the supplied ceiling and state the topology case separately. Compare all documented partitions with select_for_schema and the existing crossed schema/ceiling tests.
