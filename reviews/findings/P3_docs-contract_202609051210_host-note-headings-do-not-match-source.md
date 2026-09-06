---
id: R164-ASTRA-04
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/runner/host.md:125
provenance: pre_existing
first_bad:
guard: "PR #156 shared-host verification must cover this heading proposition explicitly: rerun fixed-string lookup for the seven misses in the independent audit.json after integration. Keep it distinct from Pages backlink navigation."
---

## Failure sequence

The inherited host note promises that its headings are source grep strings. A reader copies HostRunner::resolved at line 125 into fixed-string search of src/runner/host.rs and gets no match. The same failure affects headings at lines 153, 160, 170, 423, 429 and 434. The independent audit.json records all seven misses. The seven event-model notes pass their source-fragment audit; these misses belong to the inherited host pilot. This finding concerns heading-to-source lookup, not whether the separate source backlink works on GitHub Pages.

The reviewer established that the heading exists at b684d565b365f1c83d4a2ebedf8e0a4e04a9cf72. That is presence evidence; the exact first bad commit was not established.

This is finding R164-ASTRA-04 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

In the shared-host scope owned by PR #156, use source fragments with their enclosing item, or document an accurate lookup convention for this curated note. Verify this specific proposition after integration before closing the record. A Pages backlink fix or assignment to #156 alone does not resolve these seven heading misses.
