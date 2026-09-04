---
id: PR135-REVIEW3-KILL-HANDOFF-MISHANDLES-PATH-IDENTITY
severity: P2
disposition: deferred
category: correctness
pr: 135
reviewed_sha: 1bcc12e5dfffdb2745f581c19e307ef21a241db0
location: src/engine/topology/scaffold.rs:1405
provenance: pre_existing
first_bad:
guard: the sweep of src/engine/topology/scaffold.rs
---

## Failure sequence

With a valid Unix `TMPDIR` containing byte `0xff`, the parent passes `UPSTROKE_TEST_KILL_DIR` as an
exact `OsStr`, but `kill_child_environment` reads it back with `std::env::var(...).expect(...)` and
panics `NotUnicode`. Change that to `var_os` and `hand_off` still serialises the fixture root
through `to_string_lossy` before `read_to_string` reads it, so the parent adopts a path with
`U+FFFD` where the bytes were, operates on nothing, and the real tree leaks. The `.expect()` on an
environment read is a third defect at the same site.

Reproduced by the pass-3 reviewer at the exact head.

## What the change that takes this up should do

Carry the path as bytes end to end, or refuse a non-UTF-8 root explicitly rather than altering it.
This pull request touched `scaffold.rs` and then returned it byte-identical to master, so none of
these is its own; they are recorded here because a pass reproduced them and a body claiming they
had been withdrawn is what let them sit unexamined for a round.
