---
id: PR162-ASTRA-NOTES-RUSTDOC-LINKS
severity: P3
disposition: deferred
category: docs-contract
pr: 162
reviewed_sha: a408608703fa34ea4e5de857bc20dd76626ac9b6
location: docs/internals/runner/container.md:109
provenance: introduced_by_feature
first_bad: ed29b5a4b196e733aa588e5a75cb2e6a5c6d71d2
guard: docs/internals/README.md
---

# Migrated rustdoc shortcuts lose their cross-module links

Owner-authorized deferred under DOCS_FAST_TRACK.md and the missing-navigation scope of STACK_STOP_RULE.md.

## Failure sequence

Open the new container notes in a normal Markdown renderer and follow the sibling hook references at lines 109 and 110. The migrated shortcuts name Rust items, but the file supplies no Markdown reference definitions. They render as text rather than links, so navigation previously supplied by rustdoc is lost.

The container note contains 100 bracketed rustdoc-style shortcuts and no link definitions. The two example targets exist at src/rundir.rs:219 and the EffectHooks re-export in src/workspace_manager.rs:87. Rustdoc's item-name resolution is documented at [Rustdoc's item-link documentation](https://doc.rust-lang.org/rustdoc/write-documentation/linking-to-items-by-name.html). This also affects other migrated notes, including runner/host/environment.md. The module backlink alone does not resolve these sibling references.

## What the change that takes this up should do

Convert the migrated cross-module references to relative source or notes links, or provide Markdown reference definitions. Keep item names as the labels.
