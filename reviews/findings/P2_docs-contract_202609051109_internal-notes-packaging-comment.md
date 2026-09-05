---
id: PR159-PASS1-05
severity: P2
disposition: deferred
category: docs-contract
pr: 159
reviewed_sha: e41d542250447bb776d552ec7573d16dbe8b07ab
location: Cargo.toml:23
provenance: pre_existing
first_bad: 45ecb3d86c33bcdadc907d95fa5874e70418fd13
guard: Verify PR 156's packaging-comment repair in the final integrated head before resolving this finding.
---

## Failure sequence

The manifest describes internal documents as excluded website material, but its
exclusion list admits internal module notes. A reader uses that comment to assess
the published crate and gets the package contents wrong. The previous review
confirmed that the internal notes ship.

The defect is inherited from PR #144 and is absent from PR #159's current slice
diff. The original review gave no severity labels. P2 docs-contract is the current
steward triage, and the historical verdict remains CHANGES_REQUIRED.

## What the change that takes this up should do

PR #156 is the sole shared carrier for the packaging-comment correction. Preserve
the existing exclusions and describe the intended inclusion of internal notes.
Assignment and inheritance do not resolve the defect. astra_merge must integrate
the actual carrier from master, verify the corrected wording in PR #159's final
head, then remove this file and mark its permanent PR ledger row fixed. Keep both
the carrier identity and final integrated identity in the evidence.
