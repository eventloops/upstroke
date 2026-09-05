---
id: PR162-ASTRA-NOTES-MARKER-LITERALS
severity: P2
disposition: deferred
category: correctness
pr: 162
reviewed_sha: a408608703fa34ea4e5de857bc20dd76626ac9b6
location: .github/scripts/test-internals-notes.sh:56
provenance: introduced_by_feature
first_bad: e711a5c227987ea3ea93fdef8bbd9c124478f75d
guard: test-internals-notes.sh
---

# The notes gate treats an ordinary Rust string as a notes marker

Owner-authorized deferred under STACK_STOP_RULE.md. The demonstrated consequence is a false rejection caused by harmless source text, without a product regression.

## Failure sequence

Keep a module's valid single notes marker and valid backlink, then add a Rust string containing Extended notes:. The recursive grep feeds the string's source line into marker validation, and the later raw-text counts report two markers. The documentation gate rejects the file even though its real marker and module linkage are unchanged.

The exact candidate script was copied byte for byte into a private scratch fixture. The valid_control case exits 0; ordinary_literal exits 1 with both a malformed-marker diagnostic and a two-marker diagnostic. See /srv/worktrees/astra-20260905/agents/astra_review_162/evidence/notes-gate-witnesses.json and buildq job ae39630a401f4cda84ab1a1b6084d5e0. No candidate source was mutated. Standards section 12 requires source instruments to distinguish code, comments and literals.

## What the change that takes this up should do

Recognize actual Rust module-doc comments rather than every raw substring, and count only those markers. Retain a positive malformed-marker control and an ordinary-string control.
