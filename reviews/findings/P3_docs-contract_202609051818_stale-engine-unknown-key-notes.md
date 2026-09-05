---
id: PR157-ASTRA-ENGINE-KEY-NOTES
severity: P3
disposition: deferred
category: docs-contract
pr: 157
reviewed_sha: af2c3efe93673acf5a3ae0c849db5f7657ec5579
location: docs/internals/config.md:1081
provenance: pre_existing
first_bad:
guard: Future configuration-notes maintenance should distinguish unknown keys from unknown shell values
---

This inherited documentation finding is deferred under the owner's 2026-09-05
docs fast-track policy. The parser correctly refuses the input. No product
regression or prerequisite repair is claimed.

## Failure sequence

Read `docs/internals/config.md:1081`, which says an unknown `[engine]` key
warns and leaves the ceiling at its default. Line 46 makes the same distinction
between `[runner]` and `[engine]`. Supply an engine section containing
`on_task_failur = "continue"` and `max_paralel = 4`. The actual parser at
`src/config/parse.rs:451` returns a configuration error naming both keys,
before any warning or fallback. It does so on fresh runs and on both
sequential-resume readings.

The existing test
`config::parse::tests::an_unknown_engine_key_is_refused_and_named_on_every_reading`
at `src/config/parse.rs:1005` exercises this input and checks that warnings
remain empty. It passed in independent buildq job
`2751f23f16244be1ba3d99696c02a99e`. Its complete log is
`/srv/worktrees/astra-20260905/agents/astra_review_157/focused-engine-keys-af2c3efe.log`.

Both stale statements already occur in `src/config.rs` at base
`735ef2142238885041f30d82cc3409a67863a0d1`, where the parser already refuses
unknown engine keys. The migration carries that existing mismatch into the
new notes. The earliest bad revision was not established.

## What the change that takes this up should do

Correct both comparisons to say that unknown keys in either section are
errors. Keep the distinct rule for an unknown value of the recognized
`shell` key, which still warns and selects the native shell.
