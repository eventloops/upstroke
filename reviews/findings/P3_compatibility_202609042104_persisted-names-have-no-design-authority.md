---
id: SWEEP-NAMES-008
severity: P3
disposition: deferred
category: compatibility
pr: 143
reviewed_sha: b81c101e8d9325c359a4c30472c048567abd4132
location: src/rundir/names.rs:35
provenance: pre_existing
first_bad:
guard: the owner, in `DESIGN.md` §15; no session on this box edits `DESIGN.md`
---

## Failure sequence

Six of the eight names in `src/rundir/names.rs` are **persisted on-disk names with no authority in
`design/`**: `.creating`, `.creating.tmp`, `owner.json`, `owner.json.tmp`, `committed.json`,
`committed.json.tmp`. `DESIGN.md` §15's run-directory drawing carries the other two,
`events.jsonl` and `plan.normalized.json`, and nothing else in `design/` names any of the six.

So the repository states no rule about renaming them, and for these six a rename is not a
refactor. `committed.json` is the concrete case, and the sequence is:

1. A run is killed after old P5b. Its private half holds a file named `committed.json` — the one
   file whose existence is the deletion boundary (`src/engine/topology/create.rs`: "There is
   exactly one, it is the existence of `<private>/committed.json`").
2. Someone renames `COMMIT_RECORD`. This is a well-behaved change: every call site moves with the
   constant, and `the_names_on_disk_are_the_names_the_packet_writes` moves with it too, since it
   pins the constant to a literal and both are edited together. The suite is green.
3. A later `startup_census` reaches that husk and runs `prove_private_half_ownership`. Conjunct 12
   stats the **new** name (`src/rundir/ownership.rs:245`, `fs::symlink_metadata(locator.join(
   COMMIT_RECORD))`) and gets `NotFound`.
4. `commit_record_proves_absence` answers `true` on that `NotFound`, so the proof reaches
   `PrivateHalfOwnership::Proven` and mints a `PrivateHalfProof`.
5. That token authorises `rundir::remove_private_husk`. A private half that **had** crossed the
   deletion boundary is deleted.

The fail-closed stat is doing exactly what it was designed to do; what has gone wrong is that the
question it asks — "is there a file with this name?" — silently changed meaning between the
version that wrote the directory and the version reading it. `CODING_STANDARDS.md` §8 states the
rule this crosses: "On-disk data is untrusted, including data written by an older or interrupted
version: validate schema, bounds and invariants before constructing domain state."

Nothing in `src/` can close this. A retired decision record establishes no product contract, and
neither does implementation or test code quoting one — which is why
`decisions.workspace_candidates.run_creation` is no longer cited in
`src/rundir/names.rs`, and why the pinning test is described there as pinning the byte string and
nothing with design standing.

## Why this is recorded rather than fixed

The remedy is a compatibility contract in `DESIGN.md` §15 — at minimum, which of these six names
are frozen, and what a reader must do when it meets a run directory written under an older
spelling. `DESIGN.md` is the owner's and no session on this box edits it, so this pull request
cannot carry the fix. The coordinator ruled it deferred on 2026-09-04.

Severity is P3 because the trigger is a deliberate rename that nobody has made and that the
repository currently gives no reason to make; the defect is a missing rule, not a live wrong
answer. **If a later pass labels this P1 or P2, the disposition is not "still deferred":** it
becomes escalate-to-owner, because the fix is out of every session's reach rather than merely out
of this pull request's scope.

## What the change that takes this up should do

Add to `DESIGN.md` §15 a sentence per frozen name, or one rule covering the six, saying that these
are wire names of a persisted format and what compatibility they carry. Then the deletion boundary
can say what it means across a rename: either the six are frozen outright, or conjunct 12 stats
every historical spelling and only a run directory that holds none of them proves absence.

Whoever takes it should note that this is one instance of a wider class the coordinator is
drafting for the owner: code citing retired decision records that have no destination in
`DESIGN.md`'s retired-records table. `workspace_candidates` is one such record, and this finding is
the sharpest consequence found so far — the citation gap reaches a deletion.
