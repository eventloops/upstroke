---
id: PR159-ASTRA-002
severity: P3
disposition: deferred
category: docs-contract
pr: 159
reviewed_sha: 6dffb9808405e41ce3d36c91a7ced879a59a3af2
location: docs/internals/status.md:15
provenance: introduced_by_feature
first_bad: a520ff96fe6b1e1120f7b058e80482649b9a0565
guard: Validate ordinary Markdown link destinations when migrating Rustdoc; resolve RunState::apply to its source or notes section.
---

## Failure sequence

A reader opens the status notes in GitHub and follows the RunState::apply link.
Its destination is crate::events::RunState::apply, which Rustdoc could resolve
beside the original Rust item but ordinary Markdown cannot resolve to repository
code. The method is in src/events/mod.rs at line 1165 of the reviewed candidate.

The independent review's notes-audit.json records the destination and exact
location. The file's top-level source backlink still resolves. This is a P3
navigation defect under standards §4 and §13, deferred under the owner's docs
fast-track direction.

## What the change that takes this up should do

Use an ordinary relative link to the source or an existing matching notes
section. Check explicit link destinations when moving Rustdoc into standalone
Markdown. Preserve this finding's stable ID in the PR body's canonical ledger.
