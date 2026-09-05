---
id: ASTRA165-002
severity: P2
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: src/runner/host.rs:65
provenance: pre_existing
first_bad: PR156-SHARED-HOST-PROTOCOL
guard: PR156-SHARED-HOST-PROTOCOL must compare adjacent protocol wording with both lock intervals, poisoning recovery, and scope-exit release
---

## Failure sequence

`HostRunner` keeps mutex fields at lines 69 and 70 while the concurrency protocol exists only in `docs/internals/runner/host.md`. The adjacent reasoning required by standards sections 10 and 13 is absent at the type, leaving a reader at the lock site without the actual protocol.

## What the change that takes this up should do

The #156 carrier should state that `program_for` holds `resolved` through lookup,
resolution, and insertion at source lines 165 through 179. `run` returns from that
operation before taking `hooks` at line 195 and then holds `hooks` through subprocess
supervision. Both sites recover the inner state after poisoning; guards release on
scope exit. Put this protocol beside the type and correct the notes' claim that
`hooks` covers the whole `run`.

The review retains the source excerpts in `finding-source-excerpts.json`.
This is the section 10 site obligation retained by section 13, independent of the
transitional section 6 and 7 sweep rules. Prior row PR165-SHARED-003 and carrier
PR156-SHARED-HOST-PROTOCOL refer to this finding. The carrier assignment remains
pending actual integrated repair evidence.
