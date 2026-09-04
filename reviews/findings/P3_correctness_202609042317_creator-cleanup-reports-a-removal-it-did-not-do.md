---
id: PR139-PASS4-CREATOR-SWALLOWS-REMOVAL-ERROR
severity: P3
disposition: deferred
category: correctness
pr: 139
reviewed_sha: 913cf46eab1a8ae26d4680458bb8b4422665ca3f
location: src/engine/topology/create.rs:1474
provenance: pre_existing
first_bad:
guard: the sweep of `src/engine/topology/create.rs`, or whichever change next touches `stat_after_error`; the census side of the same call already surfaces the error as `RunDirOutcome::Unreclaimable` and is the shape to copy
---

## Severity, and whose it is

**P3, assigned by PR #139's author; review pass 4 assigned none.** The verdict that raised this
carried no severity, so the label here is the author's and was answered under rule 2 with a
measurement rather than asserted. The sentence that carries it: **at PR #139's head this is a false
operator report over correct on-disk state that the next census completes.** The listing refuses
before anything is touched, the marker survives, and the next census's proof answers `TargetAbsent`
and either reclaims the husk once it is listable or reports `Unreclaimable` while it is not — both
already witnessed in that pull request. A later pass that relabels this P2 has that measurement to
answer, not a bare row: it would need to show an on-disk consequence at this head, and there is
none. (At master before #139 there *was* one — the marker was unlinked — which is why the row
existed as P2 in the first place and why it is P3 now.)

## Failure sequence

`stat_after_error`, the creator's abort path, calls `remove_public_husk` under `let _ =` in both of
its reclaiming arms and then returns `Disposition::PublicHalfRemoved(shape)` or
`Disposition::BothHalvesRemoved { .. }` unconditionally. Both variants say the public directory
was reclaimed, and `removed_anything()` is `true` for them.

1. A creator reaches P1 — `.creating` published, no private target — and P2 fails, so the abort
   path runs.
2. The public directory is searchable but not listable (`--wx` on Unix; a directory whose read
   permission a mount or an operator removed).
3. The proof reads the marker, establishes the locator, stats the target, and answers
   `NothingBound(TargetAbsent)` — correctly.
4. `remove_public_husk` is called and its listing fails.
   - **At PR #139's head:** it returns `UpstrokeError::Io` naming the directory before touching
     anything. The directory and the marker are intact.
   - **At master before #139:** `read_dir_names` answered an empty listing, the loop removed
     nothing, the marker was unlinked, and `remove_dir` failed on the non-empty directory. Measured
     in #139's round-1 mutation as `the marker that locates the private half survived: …
     Directory not empty (os error 39)` — the mutation is master's behaviour under #139's test.
5. Either way the `Result` is discarded and the creator reports "the public run directory was
   reclaimed". At master it also lost the marker while saying so.

So the report was false before #139 and is false after it; what #139 changed is that the marker
now survives the failure. #139's body originally claimed the listing error "reaches the caller";
review pass 4 found that it does not reach this one, and the claim is corrected there.

## What the change that takes this up should do

The repair shape is costed here so that a P2 ruling can be acted on without re-deriving it:

Stop discarding the result. `engine::topology::startup::apply` handles the identical call on the
census side by turning the error into `RunDirOutcome::Unreclaimable { step, detail }` — a fifth
outcome rather than a false one — and the creator's `Disposition` wants the same arm: a variant
that says the public half is still there and why, so `removed_anything()` is false for it and the
operator-facing sentence does not claim a removal that did not happen. The best-effort comment
above the call ("a public removal that failed leaves a husk whose marker names an absent target,
which the next census reclaims") describes what happens on disk correctly; it is the *report* that
is wrong, and a report that says "left for the next census" is both true and useful.

Not done in #139 because `src/engine/topology/create.rs` is not that pull request's file — it took
one doc comment there and nothing else — and because the repair is a new `Disposition` variant with
its own `describe()` sentence and its own consumers, which is that file's change to make.
