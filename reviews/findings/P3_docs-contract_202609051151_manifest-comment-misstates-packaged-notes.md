---
id: PR164-PASS1-05
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 396a28b3d47f672a836eb79018e80b32e4198719
location: Cargo.toml:23
provenance: pre_existing
first_bad: 45ecb3d86c33bcdadc907d95fa5874e70418fd13
guard: "PR156-SHARED-PACKAGING in PR #156 owns the manifest-comment correction; retain package exclusions and verify the comment in the integrated final head before closing this finding."
---

## Failure sequence

The inherited manifest narrows the docs exclusion to docs/index.html and docs/CNAME. Adding the family notes therefore includes them in the source package, while the adjacent comment still describes docs as website material excluded from the library. The original PR body did not disclose this artifact change. A reader relying on that comment and body gets the wrong account of the package contents.

This is original finding 5 in the [independent review of PR #164](https://github.com/eventloops/upstroke/pull/164#issuecomment-5548866464). That reviewer reported nine note files and 316,021 raw bytes for its stacked reviewed diff. Those are historical review measurements, not a new package listing. The original review gave no severity labels; P3 and pre_existing are the author's classifications. The shared canonical mapping is PR156-SHARED-PACKAGING, whose provisional classification does not replace this row's recorded classification.

The published body for candidate 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6 now discloses this family's seven included notes and docs/internals/events/log.md as the compile-fail fixture test's runtime input. That addresses the body omission. The manifest comment remains unchanged, so the combined finding remains deferred.

## What the change that takes this up should do

PR #156 should correct the manifest comment to distinguish included module notes from excluded working and website assets. Preserve the existing exclusion list and all fixture inputs. After astra_merge integrates the carrier, verify the final manifest comment and retain the family's package disclosure in the PR body. Assignment to #156 does not close this record. Delete it only when the permanent PR ledger records the verified fix.

## Independent repeated observation

Alias R164-ASTRA-06, reviewed at 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6, location Cargo.toml:23, is the same unresolved proposition. It shares this individual record and retains a separate canonical ledger row so neither observation is lost. The independent reviewer assigns P3 and inherited provenance, agreeing with the original author's classification. Its cargo package --list result includes all nine internal-note Markdown files, including events/log.md as the fixture input. Its package-list.log supplies an independent package witness. The review confirms narrowing at 45ecb3d86c33bcdadc907d95fa5874e70418fd13 and notes that the corrected PR body does not repair the unchanged manifest comment.

The complete independent GPT-6 Astra/max review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb. It returns PASS WITH FINDINGS in prose and VERDICT: PASS. Under the owner's docs-first amendment this documentation finding remains deferred, including its placement obligation where applicable. Shared ownership is not evidence of integration or repair.
