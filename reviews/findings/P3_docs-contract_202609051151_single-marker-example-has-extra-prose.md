---
id: PR164-PASS1-02
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 396a28b3d47f672a836eb79018e80b32e4198719
location: docs/internals/README.md:40
provenance: pre_existing
first_bad: 929019ca455f76f95c3b4f7fcedf8f02bbcb1638
guard: "PR156-SHARED-EXAMPLE in PR #156 owns the single-marker example and accurate exception/enforcement description; verify its actual integration against standard 13 before closing this finding."
---

## Failure sequence

A contributor follows the README's instruction to put one marker in the module header and nothing else. Its example instead includes a module description and a blank rustdoc line before the marker. Copying that example adds source prose that standard 13 requires in the notes file. The contradiction remains at candidate 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6.

This is original finding 2 in the [independent review of PR #164](https://github.com/eventloops/upstroke/pull/164#issuecomment-5548866464). The original review gave no severity labels; P3 and pre_existing are the author's classifications. The shared carrier uses PR156-SHARED-EXAMPLE for the same valid proposition. Its provisional classification does not replace this row's recorded classification.

The separate claim that N1 through N4 promise to reject all arbitrary source prose is rejected under PR164-PASS1-02-SCANNER in the PR body's ledger. That claim is not part of this open finding and does not invalidate the README contradiction.

## What the change that takes this up should do

PR #156 should reduce the example to the single pointer, account for the existing site-specific exceptions, and describe the gate's structural enforcement boundary accurately. No global prose scanner is requested. After astra_merge integrates the carrier, verify the actual final README against standard 13 and the gate's stated checks. Assignment to #156 does not resolve this record. Delete this file only when that verification supports a fixed disposition in the permanent PR ledger.

## Independent repeated observation

Alias R164-ASTRA-05, reviewed at 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6, location docs/internals/README.md:43, is the same unresolved proposition. It shares this individual record and retains a separate canonical ledger row so neither observation is lost. The independent reviewer also identifies the absolute no-comments wording at lines 10 and 11, which must acknowledge the already-listed placement exceptions. The reviewer assigns P3 and inherited provenance, agreeing with the original author's classification. The conflicting example is present at 929019ca455f76f95c3b4f7fcedf8f02bbcb1638. The guard remains comparison with standard 13 and the four stated exceptions.

The complete independent GPT-6 Astra/max review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb. It returns PASS WITH FINDINGS in prose and VERDICT: PASS. Under the owner's docs-first amendment this documentation finding remains deferred, including its placement obligation where applicable. Shared ownership is not evidence of integration or repair.
