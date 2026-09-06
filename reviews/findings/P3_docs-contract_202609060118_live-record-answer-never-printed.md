---
id: PR173-LIVE-RECORD-ANSWER-NEVER-PRINTED
severity: P3
disposition: deferred
category: docs-contract
pr: 173
reviewed_sha: 429afd082e5628e8131627bc822dcc882de29aed
location: src/runner/host/tests.rs:2470
provenance: introduced_by_feature
first_bad:
guard: the next push to PR #173, or the next revision of its body
---

## Failure sequence

The body says the next observation of a running child on the macOS leg will show whether `proc_pidinfo`'s non-zero argument answers for a live process -> `group_leadership` renders the records only into a failing assertion's message, and a passing grid prints nothing -> the promised measurement cannot happen as described. Beside it, "no assertion was weakened" overstates: the expected vector is unchanged, but the predicate behind it was widened to accept the exited-unreaped state.

## What the change that takes this up should do

Either print the records on the passing path (a `--nocapture` run would then carry them; CI's does not) or, more honestly, drop the claim and state the open question as open: the answer is on the macOS runner, one experiment away. Restate the assertion sentence as "the expected value is unchanged; the predicate now also accepts an exited, unreaped child whose record names its own pid".

Recorded from the frontier pass of 2026-09-06 (`gpt-5.6-sol`, max effort) on PR #173 at `429afd0`, posted as https://github.com/eventloops/upstroke/pull/173#issuecomment-5556000412. Filed as the reviewer wrote it, with the author's reading beneath.
