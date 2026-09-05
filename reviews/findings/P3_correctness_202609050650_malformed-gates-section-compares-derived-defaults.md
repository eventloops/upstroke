---
id: SWEEP-CONFIG-PARSE-026
severity: P3
disposition: deferred     # the typed state lives in Config (row 54) and its reader in engine/preflight.rs; this file's own answer is the warning that disowns the comparison
category: correctness
pr: 150
reviewed_sha: db1591458e71675bb33d22000e002f152a3097f1
location: src/config/parse.rs:472
provenance: introduced_by_feature
first_bad: SWEEP-CONFIG-PARSE-022
guard: queue row 54 (`src/config.rs`) for the `Config::gates` state, with `src/engine/preflight.rs`'s comparison reading it; a later pass that labels this P1 or P2 fixes it there rather than re-deferring
---

## Failure sequence

A run whose log records a custom `lint` gate is resumed after today's `upstroke.toml` was
mistakenly rewritten as `[gates]` (a table) with `check = "cargo check"` -> under the
compare-only reading `parse_gates` announces that the section is not a list and returns
`Ok(None)`, the only shape `Config::gates` has for "no list" -> `validate::analyze_captured`
reads `None` as an absent section and derives the repository's default gates (`check`,
`test` for a Rust repository) -> `preflight_with_recorded` compares those derived gates with
the record and warns that `lint` was removed and `check`/`test` are "in today's config" ->
the recorded `lint` gate runs, correctly, but the operator reads edits nobody made — the
diagnostic class PR #150 set out to remove (`SWEEP-CONFIG-PARSE-004`).

PR #150 narrows it in the file: a second warning at the same point says the section cannot be
compared and that any difference reported below is between the record and the derived defaults,
not the file. The fabricated difference is still printed after it.

## What the change that takes this up should do

Give `Config::gates` a third state — the section was present and unreadable — or carry the
compare-only problems as typed data beside the list (`GateShapeProblem`, one per announced
shape), and have `preflight_with_recorded` skip `gates_differ` when today's section could not
be read, saying so once. Land it with the engine's own composition test: a recorded run resumed
through a `[gates]` table must complete on the recorded gate with exactly one warning about the
section and none about drift.
