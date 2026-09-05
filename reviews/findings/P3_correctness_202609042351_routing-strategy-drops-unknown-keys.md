---
id: SWEEP-CONFIG-PARSE-008
severity: P3
disposition: deferred     # out of scope for row 52; the type and its reader are in src/config.rs (row 54)
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config.rs:157
provenance: pre_existing
first_bad:
guard: queue row 54 (`src/config.rs`), together with `SWEEP-CONFIG-PARSE-007`
---

## Failure sequence

`[routing.strategy]` contains `mode = "value-max"` and `spend_down_afer = 0.7` ->
`RawStrategy` carries neither `deny_unknown_fields` nor an unknown-key collector -> the
misspelled key is dropped -> `Strategy.spend_down_after` is `None` and nothing says so.

P3 rather than P2 because the strategy is today "echoed, never acted on in validate" (the
parent's own warning text for an unknown `mode`), so the dropped key changes no run; the
moment spend-down routing ships (`DESIGN.md` §21, capacity-driven routing) it becomes a
silently deleted control of the same class as `SWEEP-CONFIG-PARSE-007`.

## What the change that takes this up should do

`#[serde(deny_unknown_fields)]` on `RawStrategy`, refusing by name, on the same rule the
other sections follow; a test in the parent's suite for the misspelled key, witnessed against
removing the attribute. Close it with `SWEEP-CONFIG-PARSE-007`: one attribute each, one test
each.
