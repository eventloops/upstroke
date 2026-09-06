---
id: PR157-PASS1-05
severity: P2
disposition: deferred
category: docs-contract
pr: 157
reviewed_sha: 134f426ea8a6fd52ac7d1e9d7e990d28aad83243
location: docs/internals/README.md:40
provenance: pre_existing
first_bad: 929019ca455f76f95c3b4f7fcedf8f02bbcb1638
guard: PR156 carries the shared README/gate contract repair; astra_merge verifies its actual integration ancestry and gate evidence before resolving this finding
---

## Failure sequence

The internal-notes README instructs a reader to keep one marker in the module
header and nothing else. Its example then adds two other module-comment lines.
A contributor can copy that example and pass the pointer/file gate while
violating the prose-placement rule stated in standards section 13.

This contradiction was inherited from PR144. Its merge did not resolve the
finding. The gate enforces pointer and notes-file structure; it does not claim
to scan all source prose. The separate assertion that its declared checks
promise exhaustive prose enforcement is rejected in PR157-PASS1-05-GATE in
PR157's ledger. That rejection does not dispose of this README contradiction
or the separate usable-backlink defect owned by PR156.

## What the change that takes this up should do

PR156 is the agreed carrier for the shared README/gate repair. Correct the
example, document the site-local exceptions and state the gate's actual
enforcement boundary. Preserve the independent review and gate evidence for
that repair.

Keep this finding open until astra_merge integrates the actual carrier from
master, verifies its presence in the final ancestry and checks the repaired
contract on the final head. Then record the carrier and integration evidence
in PR157's permanent ledger and delete this file. Assignment alone is not a
fix, and PR157 must not duplicate the shared gate or README edits.
