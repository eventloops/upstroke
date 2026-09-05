---
id: ASTRA165-007
severity: P3
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: docs/internals/README.md:40
provenance: pre_existing
first_bad: PR156-SHARED-EXAMPLE
guard: PR156-SHARED-EXAMPLE must compare the pointer example and general rule with retained SAFETY, concurrency, and governance obligations
---

## Failure sequence

The README says a module with notes keeps one pointer and "nothing else," while
its three-line example retains a module description and the opening rule omits
the required SAFETY, concurrency, and governance exceptions. The host notes also
conflict with the allowlist-placement marker at `src/runner/host.rs:3`, leaving
migration guidance contradictory.

## What the change that takes this up should do

Provide a single-pointer example and state the retained exceptions explicitly, including the governed allowlist marker and other at-site obligations.

The review established the conflicting example at README lines 43 through 45,
the opening rule at line 10, and the HostRunner notes at line 4 by inspection.
Prior row PR165-SHARED-002 and carrier PR156-SHARED-EXAMPLE refer to this finding.
The carrier assignment remains pending actual integrated repair evidence.
