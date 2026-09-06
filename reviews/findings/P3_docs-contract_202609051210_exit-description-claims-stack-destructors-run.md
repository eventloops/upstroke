---
id: R164-ASTRA-09
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/events/log.md:278
provenance: pre_existing
first_bad:
guard: "Compare the explanation with the Rust process::exit API documentation and the termination semantics the crash witness intends to exercise."
---

## Failure sequence

A maintainer uses the crash-test note to choose a termination mechanism or reason about RAII cleanup. The note says panic and exit both run destructors, then uses that distinction to explain abort. The independent review cites the Rust process::exit contract: it skips stack destructors, although registered exit handlers can run. The note gives the wrong account of the exit case.

The reviewer identified a pre-existing Rustdoc claim copied by f45e76a3037657b28686e462106f7d12e670dcfa. The original first bad commit was not established.

This is finding R164-ASTRA-09 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

Keep the abort implementation. Explain the distinctions between panic unwinding, process exit handlers and abrupt termination using the [Rust process::exit documentation](https://doc.rust-lang.org/std/process/fn.exit.html) and the crash witness's intended semantics.
