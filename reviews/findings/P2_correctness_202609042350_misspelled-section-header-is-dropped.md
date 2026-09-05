---
id: SWEEP-CONFIG-PARSE-007
severity: P2
disposition: deferred     # out of scope for row 52; the type is read by src/config/read.rs (row 53) and declared in src/config.rs (row 54)
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/config.rs:31
provenance: pre_existing
first_bad:
guard: queue row 53 (`src/config/read.rs`) or row 54 (`src/config.rs`), whichever sweeps first; a later pass that labels this P1 or P2 for a change in either file fixes it there rather than re-deferring
---

## Failure sequence

`upstroke.toml` contains `[budgts]` with `run_usd = 15.0`, or `[interation]` with
`mode = "never"`, or `[runer]` with `kind = "container"` -> `read_repo_config`
deserialises it into `RawRepoConfig`, which carries neither `deny_unknown_fields` nor an
unknown-key collector -> the whole section is dropped with no warning and no error ->
`Config.budgets` is `Budgets::default()` (no ceiling), `interaction_mode` is `on_block`
(a CI run that stops to ask a person), `runner` is the host default -> `upstroke validate`
reports a clean config, and the run spends against no ceiling, blocks on a human in CI, or
executes gate code on the host while the file reads as though it were confined.

Every section reader below the top level now refuses or names an unknown key (`[runner]`,
`[budgets]`, `[interaction]`, `ask_before`, `[[gates]]` entries error; `[engine]` and
`[routing]` kinds warn). The top level is the one place a typo still deletes a whole section
silently, and it deletes the largest unit.

## What the change that takes this up should do

Add `#[serde(deny_unknown_fields)]` to `RawRepoConfig`, or a `#[serde(flatten)]` collector
that refuses by name with the seven accepted sections listed (`routing`, `pins`, `gates`,
`engine`, `interaction`, `budgets`, `runner` -- exactly the fields of the struct, and
exactly the sections `design/17` documents for the repo-level file, so no forward-compatibility
key is lost). Test it in the parent's suite through `load` with a misspelled section header,
and witness the test against removing the attribute. `RawStrategy` has the same gap one level
down (`SWEEP-CONFIG-PARSE-008`) and can be closed in the same change.
