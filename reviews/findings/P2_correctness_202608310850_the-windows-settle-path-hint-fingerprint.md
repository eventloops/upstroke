---
id: PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT
severity: P2
disposition: deferred
category: correctness
pr: 104
reviewed_sha:
location: src/topology/registry.rs
provenance: undetermined
first_bad:
guard: project owner / the slice that next opens the Windows engine::topology::settle harness or the frozen path-hint derivation
---

## Failure sequence

One Windows (`test (winguest)`) run produced two `engine::topology::settle` failures together, both refusing a log replay with `MalformedEntry { kind: "task_dispatched", key: 0 }` on a predicted-region mismatch: the entry takes the predicted region `` `src/aleph/` `` while the same entry's frozen path hints derive `` `src/aleph` `` — the two strings differ only by the trailing `/`. The writer of the differing spelling is unresolved and no rate has been measured

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the Windows `engine::topology::settle` harness or the frozen path-hint derivation.

**Open as one unexplained run, not classified as a flake or regression.** **The failure.** Run `33779292591`, attempt 1, job `100728982300`, at `a5d1e14`. `engine::topology::settle::tests::retained_generation_not_continued_after_kill` panicked at `src\engine\topology\settle\tests.rs:1807:60` and `engine::topology::settle::tests::kill_after_failed_settlement_rematerializes_question` at `:1764:56`, both on `the log replays`; `test result: FAILED. 1760 passed; 2 failed; 35 ignored`. The fixture declares the region **with** the slash — `task_of("aleph", "src/aleph/", Tier::Mid)` at `settle/tests.rs:100` — and the hints derive it **without**. **It is intermittent on one leg, and the window is stated rather than summarised.** `test (winguest)` concluded **success** in runs `33776180623` (attempt 1, at this same SHA `a5d1e14`), `33774140883`, `33770200867` and `33769228836`, and **failure** in `33779292591` (attempt 1, also `a5d1e14`). So the identical commit passed this leg once and failed it once. Every run named here is `attempt=1`: no rerun-in-place is hiding a conclusion inside a row, which the API reports only at its latest attempt. **All runs at the head were read, not the latest** — reading the latest is how this class hides. **Why PR #104's diff cannot reach it, offered as reasoning that does NOT satisfy a floor.** The pull request's only `src/` changes outside `src/plan/` are in `src/topology/registry.rs`, and that change is **inside `mod tests`**, replacing `fs::read_to_string` of three fixture paths with the compile-time corpus constants; the two failing tests are in `engine::topology::settle` and read no corpus; and the shape is absent from recent failing runs on `master` and on the four sibling refactor branches. **The merge proceeded on the project owner's disclosed decision of 2026-09-03, not because this explanation cleared the red.** A floor is not satisfiable by explanation, and this row exists so that the decision is on the record with its evidence rather than resolved by argument. **What would settle it.** A spelling that passes four times and fails once is nondeterministic path handling on the Windows leg, not a fixed normalisation defect — a fixed one would fail every time. The measurement is: establish which writer produces the frozen path hints for a `task_dispatched` entry and whether its derivation is order- or environment-dependent on Windows, then run that derivation repeatedly on the winguest host against the `src/aleph/` fixture and record the distribution. **The cause is not guessed here.** **Provenance is inline rather than in a companion record**, unlike `PR43-*`, because this append is deliberately confined to `reviews/FINDINGS.md` — `decisions/2026-08-20-review-invalidation-scope.md` makes that one path the exempt set, and a second file would forfeit the exemption and invalidate the pull request's frontier review.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
