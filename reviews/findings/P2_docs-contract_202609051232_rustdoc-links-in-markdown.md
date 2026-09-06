---
id: PR161-ASTRA-RUSTDOC-LINKS
severity: P2
disposition: deferred
category: docs-contract
pr: 161
reviewed_sha: 976eae7b49e10b3560a96d2c28eb343c82cea016
location: docs/internals/effects/tests.md:65
provenance: introduced_by_feature
first_bad: 5a864e153c2b290014ed42866fbd9ac2b921e54f
guard: Convert Rustdoc references when the effects notes next receive a navigation update
---

Deferred by owner authorization on 2026-09-05 under DOCS_FAST_TRACK.md and
STACK_STOP_RULE.md. This record preserves the finding without claiming a fix.

## Failure sequence

A reader opens the migrated notes as Markdown and follows the `census_domain` link at `docs/internals/effects/tests.md:65`. Its destination is `crate::effects::census_domain`, which requires Rustdoc's item resolver and does not name a repository document or web page. The same problem affects `PACKET_PRIMITIVES` at `docs/internals/effects/tests/contract_mappings.md:123`.

Shortcut references also remain in Rustdoc form without Markdown reference definitions. For example, the `normalize_lint` reference at `docs/internals/effects.md:160` renders as bracketed code, with no link. Moving these references out of Rustdoc removes their navigation behavior even though the code headings themselves can be found by searching the source.

The independent CommonMark rendering audit found two `crate::` link destinations and 145 unresolved bracketed code references across the ten effects notes. It rendered the examples as `<a href="crate::effects::census_domain">` and `[<code>normalize_lint</code>]`. The complete inventory and renderer output are retained with the independent review evidence.

## What the change that takes this up should do

Replace Rustdoc item links with relative Markdown links to the corresponding notes or source, using an anchor where useful. Render representative cross-file and same-file references and check the resulting destinations. Use plain code spans for references that are deliberately not links.
