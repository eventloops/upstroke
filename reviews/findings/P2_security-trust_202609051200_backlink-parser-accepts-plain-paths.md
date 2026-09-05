---
id: ASTRA165-001
severity: P2
disposition: deferred
category: security-trust
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: .github/scripts/test-internals-notes.sh:102
provenance: pre_existing
first_bad: PR156-SHARED-N3
guard: PR156-SHARED-N3 must exercise the complete notes gate with hidden and non-link decoys, a real backlink, and missing and wrong-path controls
---

## Failure sequence

N3 extracts any parenthesized `.rs` path, including plain prose, fenced text, inline text, or image text, without requiring a usable Markdown backlink. A notes file containing `<!-- (../../../../src/topology/fold/check_end.rs) -->` therefore makes the complete gate exit 0 even though no rendered link exists.

## What the change that takes this up should do

The #156 carrier should require a usable backlink, retain the full negative and positive fixture set, and prove that decoy paths do not satisfy the gate.

The independent reviewer ran the complete, unmodified gate, SHA-256
`87a461481a9ea0a4717ca5e41f21902081aa8dd82d965edd9bcd6b210d23917d`.
The real-link control passed; missing and wrong-path controls failed.
Evidence retained with the PR #165 review is `evidence-9699c273/backlink_witness.py`,
`backlink-witness.json`, `backlink-witness.log`, and rendered fixture HTML.
This record carries prior row PR165-SHARED-001 and carrier PR156-SHARED-N3.
The carrier assignment remains pending actual integrated repair evidence.
