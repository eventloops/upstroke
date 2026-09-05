---
id: SWEEP-CONNECT-RENDER-014
severity: P3
disposition: deferred
category: docs-contract
pr: 168
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/connect.rs:375
provenance: pre_existing
first_bad:
guard: the owner, in `DESIGN.md` §17; the sentence "Not decided here" in `design/17_design_configuration_reference.md` marks the place
---

## Failure sequence

`~/.upstroke/pools.toml` is a persisted format: `upstroke connect` writes it, `config` reads it on
every `run`, `validate` and `capacity`, and `connect` reads it back on its next run to carry the
operator's `profile`, `monthly_allowance` and `endpoint` across a `--force` — **by pool name**
(`src/connect.rs`, `carried.get(&pool.name)`). Since PR #168, `DESIGN.md` §17 describes what
`connect` writes: the keys, the default pool name (the agent's id, `default_pool_name`), the
comments and the rewrite rules. What it says it does not decide, in so many words, is whether
those keys and that name are frozen across versions. Nothing else in `design/` decides it either:
§13's discovery paragraph says `connect` "writes the user-level pools file" and §18's command list
says "writes ~/.upstroke/pools.toml"; §17's own example names its Claude pool `claude-max`, which
is not what `connect` writes and is presented as an operator's file.

The sequence the missing rule leaves open, with the name as the concrete case:

1. `connect` v0.1 writes `[pools.claude-code]`; the operator adds `profile = "work"`.
2. A later version changes `default_pool_name` — to `claude`, say. Nothing in the repository says
   it may not.
3. The next `connect` reads the operator's keys under `claude-code`, derives a pool named `claude`,
   carries nothing into it, finds the settings differ and refuses, printing the proposed file and
   recommending `--force`.
4. `--force` writes `[pools.claude]` with no `profile`. The one setting the carrying exists to keep
   is gone, on the path the refusal sent the operator down.

Measured at `323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b` by making that rename in `default_pool_name` and running the
whole suite: **3 failed** (`a_missing_agent_skips_its_pool_without_taking_the_others_with_it`,
`what_connect_writes_parses_back_into_the_pools_it_describes`,
`an_existing_file_that_differs_is_never_clobbered`), every one because it asserts the literal
`claude-code` — a renamer edits all three with the rename. The same measurement over the keys:
renaming `endpoint` failed **0** tests at `323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b` (PR #168 adds the test that
now fails), `profile` and `monthly_allowance` 1 each, `safety_margin` and `sources` 1 each. The
guards are tests about carrying and round-tripping that happen to spell the names, not a rule.

## Why this is recorded rather than fixed

Describing what the code writes was PR #168's to do and is done (`design/17`). Deciding whether
the format is a compatibility surface — frozen keys and name, or a migration rule for a file an
older `connect` wrote — is a product decision the code does not settle, and `CODING_STANDARDS.md`
§8 asks for it ("A persisted or inter-process representation is an explicit schema ... Serde
defaults, aliases, unknown-field handling and enum tagging are compatibility decisions with
tests"). P3 because the trigger is a rename nobody has made and the repository gives no reason to
make; the defect is a missing rule, not a live wrong answer. **If a later pass labels this P1 or
P2, the disposition is escalate-to-owner, not "still deferred"**: the remedy is a decision, not
code.

## What the change that takes this up should do

Replace §17's "Not decided here" paragraph with the rule: either the keys §17 lists and the
default pool name are frozen (a rename is a new key beside the old, read for one release), or
`connect` migrates by name on read. Then `operator_keys` can carry across a rename, or refuse to,
by rule rather than by accident.
