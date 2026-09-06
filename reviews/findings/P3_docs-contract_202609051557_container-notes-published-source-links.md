---
id: PR163-ASTRA-PAGES-SOURCE-LINKS
severity: P3
disposition: deferred
category: docs-contract
pr: 163
reviewed_sha: f346f28e6be23c5f9bd3f8decfa64e025dea5c91
location: docs/internals/runner/container/runtime.md:3
provenance: introduced_by_feature
first_bad: 32e87f8170088dc19a73c04bc30184e53014bd44
guard: Future notes-link maintenance should resolve source and design links against both the repository view and the configured Pages publication root.
---

## Failure sequence

Publish the new runtime note under the repository's configured Pages root, master:/docs, and resolve its opening ../../../../src/runner/container/runtime.rs link from /internals/runner/container/runtime.md or its rendered HTML counterpart. The browser target is https://upstroke.rs/src/runner/container/runtime.rs. The published source tree has no docs/src/runner/container/runtime.rs, so the link cannot reach the referenced source through that deployment.

All 11 new notes use this repository-relative source-link pattern. The new design and capacity references at env.md lines 510 through 512, and the historical-ledger link at view.md line 339, have the same publication-root mismatch. The links do resolve in the GitHub repository view.

Evidence is the GitHub Pages API response in pages-config.json, the literal links in source-audit-v2.json, and the reviewed Git tree. The API confirms the site URL and master:/docs source. This is a deployment-path deduction; no successful live HTTP retrieval or measured HTTP 404 is claimed. The consequence is missing internal documentation navigation.

The owner authorized deferral under DOCS_FAST_TRACK.md and STACK_STOP_RULE.md. Preserve this record and its canonical ledger row before final handoff. No additional repair cycle is required for this finding.

## What the change that takes this up should do

Use revision-appropriate GitHub source links for paths outside the published docs tree, or provide an explicit publication-time mapping. Verify the resulting links from the published URL as well as from the repository view.
