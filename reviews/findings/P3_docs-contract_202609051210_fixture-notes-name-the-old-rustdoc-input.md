---
id: R164-ASTRA-02
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/events/log/tests.md:1294
provenance: introduced_by_feature
first_bad: f45e76a3037657b28686e462106f7d12e670dcfa
guard: "Compare the input path, line-number meaning and extraction description with declared_build_refusals and its runtime Markdown input."
---

## Failure sequence

A contributor follows the notes at lines 1278, 1286, 1290, 1294 and 1335 to edit doc comments in src/events/log.rs. The documented extractor supposedly strips those comment prefixes, but declared_build_refusals at src/events/log/tests.rs:2900 reads docs/internals/events/log.md directly. The contributor changes an input the test no longer reads. The claimed Rust source has zero compile-fail fences; the Markdown input has three. The independent mutation controls confirm that changes to the Markdown fixtures reach the executed test.

The reviewer identified introduction at f45e76a3037657b28686e462106f7d12e670dcfa, which changed the extractor.

This is finding R164-ASTRA-02 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

Update the documented input path, the meaning of fixture line numbers and the extraction description to match declared_build_refusals. Label any retained explanation of the old Rustdoc extraction as history. Preserve the test logic and all intended fixtures.
