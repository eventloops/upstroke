---
id: PR157-SCANNER-01
severity: P2
disposition: deferred
category: correctness
pr: 161
reviewed_sha: 787d945010bfc02d7584af6076f8fe942bc57a41
location: src/effects.rs:329
provenance: pre_existing
first_bad:
guard: PR #157 bounded configured_item_end correction with typed-wrapper and later-production regressions
---

## Failure sequence

A `#[cfg(test)]` named function returns
`Result<(RunReport, RunState), UpstrokeError>`. `production_code` asks
`configured_item_end` to find the end of the configured item. The helper tracks
parentheses, brackets and braces, but the comma after the return tuple appears at
depth zero. It returns there before reaching the function body. The test-only
body remains visible in the production census.

PR #157 witnessed the consequence at
`134f426ea8a6fd52ac7d1e9d7e990d28aad83243`, `src/effects.rs:668`.
`production_reaches_a_spawn_through_one_host_runner_per_run` counted four
constructions instead of two: two in `engine/mod.rs`, one in `engine/coordinator.rs`
and one in `engine/resume.rs`. The expected coordinator and resume counts were
zero. The peer's `census-evidence/candidate.log` records the failing assertion, and
`candidate.json` binds that log to its candidate.

The same helper is present at the local SHA and location recorded above. This
entry binds the local source inspection to that SHA; the four-versus-two test
result belongs to the peer candidate, whose ancestry is not asserted here. No new
independent approval of either candidate is claimed. The helper predates this
documentation repair, and its first bad commit remains unknown.

This finding is distinct from `PR157-CENSUS-01`. It is deferred under the owner's
documentation policy; PR #161 does not change the scanner or executable tests.

## What the change that takes this up should do

PR #157 owns the behavioral correction. Recognize the named function's return
type without allowing its generic comma to terminate the item. Preserve the
conservative fallback and configured field handling. Regressions should exclude
the typed test wrapper while retaining later production items, and the exact
census should retain its two-site expectation. A pending repair is not evidence
that this finding is fixed.
