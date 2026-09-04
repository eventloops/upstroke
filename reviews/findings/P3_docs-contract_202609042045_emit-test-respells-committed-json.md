---
id: SWEEP-NAMES-003
severity: P3
disposition: deferred
category: docs-contract
pr: 143
reviewed_sha: 7724ed1d628070b35948819095a68a38cd0c5d0a
location: src/engine/topology/emit/tests.rs:1181
provenance: pre_existing
first_bad:
guard: the sweep of `src/engine/topology/emit/tests.rs`
---

## Failure sequence

`assert!(after_p5b.paths.private.join("committed.json").is_file(), "the private
half was removed")` re-spells `rundir::COMMIT_RECORD` instead of importing it,
and it is the only path spelling of any of the six record names anywhere outside
`src/rundir` — production or test. The file two lines earlier already reaches
`after_p5b.paths`, so the constant is in reach at no cost.

The consequence is small and worth stating precisely, because the obvious
framing is wrong. This is an assertion that a file **is present**, so a rename
of `COMMIT_RECORD` makes it fail rather than pass — it does not go silently
vacuous. What it does is fail for a reason its message does not name ("the
private half was removed" when the private half is intact and the record is
simply called something else), in a test whose subject is a torn first append
and not the record's name at all.

The `src/rundir/tests.rs` decoy grid has the mirror-image shape and is
deliberate: rows named `marker+private-with-owner-record` and
`marker+private-with-commit-record` write `public.join("private/owner.json")`
and `public.join("private/committed.json")` — a `private/` directory inside the
**public** half, which is not the private half and must not be followed. Those
literals name a decoy that has to resemble a record, so they are not a
re-spelling of the constant and are left alone. They are listed here only so
the next reader does not "fix" them.

## What the change that takes this up should do

Import `crate::rundir::COMMIT_RECORD` in `src/engine/topology/emit/tests.rs`
and join it, so a rename of the constant moves this assertion with it. Leave
the two `src/rundir/tests.rs` decoy rows spelt out, and say in a comment there
why — a decoy that tracked the constant would stop being a decoy of a fixed
byte string.
