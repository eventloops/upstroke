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
guard: blocked on `SWEEP-NAMES-008`; the sweep of `src/engine/topology/emit/tests.rs` may not apply this until §15 defines wire-name compatibility
---

## Failure sequence

**Read the disposition before the finding: this row is now blocked, and applying it as first
written would remove a live safeguard.**

`assert!(after_p5b.paths.private.join("committed.json").is_file(), "the private half was removed")`
in `src/engine/topology/emit/tests.rs` spells `rundir::COMMIT_RECORD` by hand instead of importing
it. Read as a tidiness question — which is how the first version of this row read it — the repair
is obvious: import the constant and join it. Measured, that repair is the thing standing between a
`COMMIT_RECORD` rename and silent deletion of committed private data.

The measurement, at `894b24d313b775850cebaf8af5088330ff10df97`, with a
`Compiling upstroke v0.1.0 (/srv/worktrees/sweep-names)` line asserted in the log so the run cannot
be a stale binary. `COMMIT_RECORD` set to `"committed2.json"` and `COMMIT_RECORD_STAGED` with it,
and the deliberate literal-pinning test `the_names_on_disk_are_the_names_the_packet_writes` updated
the way a renamer actually would, then `cargo test --lib`:

```
test result: FAILED. 1923 passed; 1 failed; 38 ignored
failures:
    engine::topology::emit::tests::torn_first_line_is_husk_or_possibly_committed_per_commit_record
panicked at src/engine/topology/emit/tests.rs:1180:5: the private half was removed
```

**One test out of 1,924.** Not "one of the guards" — the only one. Every other spelling of the name
follows the constant and moves with it.

This row is the **canonical home** of that count. `src/rundir/names.rs`, `standards/SWEEP.md`,
`SWEEP-NAMES-008` and the pull request body point here rather than restating it, because a number
restated in five places is a number that drifts in four of them — which is exactly how
`SWEEP-NAMES-008` came to claim a green suite that this measurement contradicts. There is one
deliberate exception: the comment at `src/engine/topology/emit/tests.rs` repeats the count, because
its whole purpose is to stop a reader who has never opened this file from deleting the assertion,
and a pointer they do not follow protects nothing.

So the sequence this row would create if applied before `SWEEP-NAMES-008`:

1. A sweep of `emit/tests.rs` follows this row and changes that assertion to join `COMMIT_RECORD`.
2. Later, someone renames `COMMIT_RECORD` and updates the literal-pinning test, which is what a
   well-behaved rename looks like.
3. **The suite is green.** Nothing is left that spells the old byte string.
4. A run killed after old P5b still holds a file named `committed.json`.
5. Conjunct 12 stats the new name (`src/rundir/ownership.rs`,
   `fs::symlink_metadata(locator.join(COMMIT_RECORD))`) and gets `NotFound`;
   `commit_record_proves_absence` answers true; a `PrivateHalfProof` is minted; a private half that
   had crossed the deletion boundary is deleted.

Step 3 is what this row currently buys, and it is the whole hazard: the rename is *already* unsafe
for the reason `SWEEP-NAMES-008` gives, and today the emit literal is what makes it loud.

`src/rundir/tests.rs`'s decoy grid rows spell `"private/owner.json"` and `"private/committed.json"`
inside the **public** half. Those are a different case and also deliberate: the decoy has to
resemble a record, so it must not track the constant. They are listed here so the next reader does
not "fix" them either.

## What the change that takes this up should do

**Not the original prescription.** Two orders are acceptable and the sweep of `emit/tests.rs`
should say in its body which it took:

* **Wait.** Leave the literal exactly as it is until `SWEEP-NAMES-008` is settled and `DESIGN.md`
  §15 says what compatibility these six wire names carry. Then this row is a tidiness question
  again and the import is safe.
* **Replace before removing.** Keep a guard that names the *historical* byte string rather than the
  current constant — a test asserting that a private half written under any spelling this project
  has used is not treated as absent by conjunct 12 — and only then let the emit assertion follow
  `COMMIT_RECORD`. This is the better end state, because it guards the property rather than a
  coincidence of one test's literal, but it is `SWEEP-NAMES-008`'s contract that says which
  spellings count, so it still waits on the owner.

Either way, add a comment at `emit/tests.rs:1180` saying the literal is deliberate and pointing
here, so the next reader who greps for hand-spelled names does not remove it without reading this
row.
