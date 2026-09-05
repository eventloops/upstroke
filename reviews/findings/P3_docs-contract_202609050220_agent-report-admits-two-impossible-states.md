---
id: SWEEP-CONNECT-RENDER-008
severity: P3
disposition: deferred
category: docs-contract
pr: 155
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/connect.rs:75
provenance: pre_existing
first_bad:
guard: the sweep of `src/connect.rs` (`standards/SWEEP.md` row 57); `connect::render::tests::the_summary_and_the_file_agree_on_which_agents_are_usable` pins what the renderers do with the two extra states meanwhile
---

## Failure sequence

`AgentReport` is `outcome: Result<Discovery, String>` beside `pool: Option<Pool>`, both `pub`. The
parent sets them together — `Ok` with `Some`, `Err` with `None` — so of the four states the type
admits, two are never built by `run_with`. Both renderers had to read the pair anyway, and at
`323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b` they read it differently: `pools_file` printed "not usable" for
`(Ok, None)` and the summary printed nothing at all for the same report, while `(Err, Some)` had a
pool in hand that the file dropped without saying so. `CODING_STANDARDS.md` §5: "An enum, not
string tags or a set of booleans, encodes alternatives with different meaning."

PR #155 makes the two renderers decide once (`render::usable`) and name every agent in the summary,
so the extra states now render consistently; the type still admits them, and `connect` is a
`pub mod` of a published crate, so anything can build one.

## Why this is recorded rather than fixed

`AgentReport` is the parent's public type and the fix changes its shape, which is the parent's
sweep and a SemVer question, not a rendering one.

## What the change that takes this up should do

Replace the pair with an enum on the report — `Usable { discovery, pool }` and
`Skipped { error }`, or `outcome: Result<(Discovery, Pool), String>` — and delete
`render::usable`'s third arm, which then cannot be written. Assess the public-API change for SemVer
in the pull request.
