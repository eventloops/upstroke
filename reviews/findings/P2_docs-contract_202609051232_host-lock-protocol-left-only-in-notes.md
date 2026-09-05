---
id: PR156-SHARED-HOST-PROTOCOL
severity: P2
disposition: deferred
category: docs-contract
pr: 161
reviewed_sha: 976eae7b49e10b3560a96d2c28eb343c82cea016
location: src/runner/host.rs:69
provenance: pre_existing
first_bad:
guard: Keep the shared HostRunner documentation work with PR 156 and verify its integration
---

Deferred by owner authorization on 2026-09-05 under DOCS_FAST_TRACK.md and
STACK_STOP_RULE.md. This record preserves the finding without claiming a fix.

The independent reviewer reported this finding as `PR161-ASTRA-HOST-LOCK-PROTOCOL`. That identifier is an equivalent reviewer alias of the canonical shared `PR156-SHARED-HOST-PROTOCOL`; this file is their single record. PR #156 owns the shared work. Assignment alone does not resolve the finding.

## Failure sequence

The source migration removes the comments beside `HostRunner::hooks` and `HostRunner::resolved`. A reader of `src/runner/host.rs:65` now sees two mutex fields without their local protocol. The explanation that `hooks` is held for a complete process run and that resolution is remembered per runner lives only in `docs/internals/runner/host.md`.

The generic `Mutex` field types do not express the protected protocol or acquisition order. `program_for` holds `resolved` across lookup and insertion at `src/runner/host.rs:165`, then returns before `Runner::run` acquires `hooks` at line 195 and holds it through process supervision. Standards section 10 requires a written concurrency protocol in types, Rustdoc or an adjacent comment. Section 13, lines 15 through 17, explicitly preserves that placement obligation for modules with notes. The source-only pointer does not satisfy the exception.

This is inherited shared documentation work. The host source and notes are already present in ancestor `3a08a1f33456cba159d05f667c72d01e4320767f` and are byte-identical to observed master `813b67d44958b9066116c9509e01434884cc0276`. The PR body assigns the shared HostRunner protocol work to PR 156. No runtime change or observed deadlock is alleged.

## What the change that takes this up should do

Keep a concise protocol beside the owning type or fields. Name the state each mutex protects, the point at which an answer becomes fixed, the absence of nested acquisition, the serialization through `hooks`, and failure or poison handling. Keep the longer rationale in the notes. Preserve the shared PR 156 ownership and deduplicate this record against its record for the same issue.
