---
id: PR164-PASS1-03
severity: P2
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 396a28b3d47f672a836eb79018e80b32e4198719
location: src/runner/host.rs:69
provenance: pre_existing
first_bad: 929019ca455f76f95c3b4f7fcedf8f02bbcb1638
guard: "PR156-SHARED-HOST-PROTOCOL in PR #156 owns the adjacent HostRunner protocol; verify its actual integration against standards 6, 10 and 13 before closing this finding."
---

## Failure sequence

The inherited #144 migration moves the HostRunner field protocol into docs/internals/runner/host.md. A reader inspecting HostRunner's hooks and resolved Mutex fields then finds no adjacent account of the protected state and lock duration. The description exists only in external notes. Standard 10 requires the concurrency protocol in types, rustdoc or an adjacent comment; standard 13 preserves that placement obligation. HostRunner is already swept, so standard 6's lock rules apply to the whole file. This placement defect remains at candidate 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6. No runtime race is claimed.

This is original finding 3 in the [independent review of PR #164](https://github.com/eventloops/upstroke/pull/164#issuecomment-5548866464). The original review gave no severity labels; P2 and pre_existing are the author's classifications. At the original observation, the exact first bad commit had not been established; its ledger identified inheritance from #144 and mapped the finding to PR156-SHARED-HOST-PROTOCOL. The later independent observation below establishes the removal commit.

## What the change that takes this up should do

PR #156 should verify the actual lock uses and restore a concise protocol beside the type or fields, covering protected state, acquisition order and critical-section duration as required by standards 6 and 10. The host notes should explain the retained placement exception. Do not copy the stale claim that the whole execution system is sequential, or change lock behavior as part of this documentation repair. After astra_merge integrates the carrier, verify the final source and notes before closing this finding. Assignment alone does not resolve it. Delete this file only when the permanent PR ledger records the verified fix.

## Independent repeated observation

Alias R164-ASTRA-01, reviewed at 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6, location src/runner/host.rs:69, is the same unresolved proposition. It shares this individual record and retains a separate canonical ledger row so neither observation is lost. The independent reviewer assigns P2 and inherited shared-pilot provenance, agreeing with the original author's classification. The review establishes the protocol removal at 929019ca455f76f95c3b4f7fcedf8f02bbcb1638. Its source inspection describes per-runner resolution memo ownership, resolution finishing before the spawn lock is taken, and hooks staying locked through process supervision. Its guard is comparison with program_for and Runner::run, including guard lifetimes and acquisition order. This supplies the first-bad evidence that was unknown in the original observation.

The complete independent GPT-6 Astra/max review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb. It returns PASS WITH FINDINGS in prose and VERDICT: PASS. Under the owner's docs-first amendment this documentation finding remains deferred, including its placement obligation where applicable. Shared ownership is not evidence of integration or repair.
