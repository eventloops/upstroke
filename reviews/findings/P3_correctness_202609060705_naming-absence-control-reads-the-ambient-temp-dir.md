---
id: SWEEP-HOST-NAMING-005
severity: P3
disposition: deferred     # parked with the P2 above; the fix is known and small
category: correctness
pr: 185
reviewed_sha: e518bb782b97d3bac952b4fece737ee1b6de7d8d
location: src/runner/host/naming.rs:304
provenance: introduced_by_feature   # added by the pass-1 repair in b9c73630
first_bad: b9c73630f0228036ad9c0baeb344e36ec589ca69
guard: `a_candidate_that_is_merely_absent_is_still_absence` is the test this finding is about
---

## Failure sequence

`a_candidate_that_is_merely_absent_is_still_absence` points `PATH` at
`std::env::temp_dir()` — an ambient directory this test does not own — and asserts that the fixed
name `upstroke-no-such-program` is refused as absent. If any process or a retained fixture leaves
an executable of that name (or, on Windows, of the corresponding `PATHEXT` spelling) in the shared
temporary directory, `resolve_program` succeeds and the test fails against an unchanged
repository tree. §12 requires deterministic, hermetic tests over uniquely owned temporary state,
and a control whose oracle depends on what else is in `/tmp` is not that.

The reviewer's severity is accepted. The risk is small — the name is deliberately absurd and, on
Unix, the file would also need the execute bit — but "unlikely to collide" is not "hermetic", and
this test is part of the evidence that the file is swept.

## Why it was written that way

This module carries `#![deny(clippy::disallowed_methods, ...)]` and `clippy.toml` disallows every
`std::fs` creation primitive (`create_dir`, `create_dir_all`, `write`, `File::create`,
`OpenOptions`, `set_permissions`). A test in this file therefore cannot build an owned temporary
directory without an `effects/allowlist.toml` row, which is outside this sweep's scope. Every one
of the four tests is built from values for that reason. The sibling test
`a_candidate_this_platform_cannot_stat_is_never_reported_as_absence` is unaffected: its fixture is
an interior NUL in the path, which no filesystem state can satisfy.

## What the change that takes this up should do

The hermetic fix needs no filesystem write and no allowlist row: point `PATH` at a uniquely named
directory that is *never created*, e.g. `std::env::temp_dir()` joined with a name carrying the
process id and a per-test counter. Nothing exists at that path, so every candidate under it is a
genuine `NotFound`, the branch under test is reached identically, and no other process can
falsify it. Rename the test only if its sentence changes.
