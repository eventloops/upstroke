---
id: R164-ASTRA-03
severity: P3
disposition: deferred
category: docs-contract
pr: 164
reviewed_sha: 16e471a4ebbed78bd1be27d3b3e786d0d2e302c6
location: docs/internals/events/mod.md:268
provenance: introduced_by_feature
first_bad: f45e76a3037657b28686e462106f7d12e670dcfa
guard: "Inspect rendered HTML for the migrated cross-references, including explicit Rustdoc destinations and shortcut references; Markdown text and heading matches alone do not verify links."
---

## Failure sequence

A reader follows the new source pointer into events/mod.md and tries the related-contract references at lines 268, 318, 476 and 863. Their destinations, crate::engine::ResumeOptions, crate::gates::ShellKind, crate::ir::Outcome and RunState::apply, are Rustdoc paths. GitHub's HTML for the reviewed commit renders the code labels without anchors. Shortcut references such as RunState::apply at line 19 remain bracketed text. The reader cannot follow these references to their related contracts. The independent evidence files events-mod-rendered.html and notes-preservation.json record the rendered result.

The reviewer identified introduction by the renderer change in f45e76a3037657b28686e462106f7d12e670dcfa.

This is finding R164-ASTRA-03 from the independent GPT-6 Astra/max review of PR #164 at the reviewed SHA above. The reviewer classified it P3, docs-contract, with the provenance recorded here. The complete review has SHA256 eef7a0e7c93be03a5507ba9f504740f2b6f61941ec49627539a4347dcfcffbeb and returns PASS WITH FINDINGS in prose, with VERDICT: PASS. It does not claim this finding is fixed. The owner's 2026-09-05 docs-first amendment requires its individual record and deferred ledger disposition.

## What the change that takes this up should do

Convert intended references to Markdown links that resolve to source or notes. Use plain code spans where no link is intended, and inspect rendered HTML to confirm the selected destinations work.
