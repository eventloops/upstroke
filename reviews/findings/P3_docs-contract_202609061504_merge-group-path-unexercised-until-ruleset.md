---
id: PR207-MERGE-GROUP-PATH-UNEXERCISED
severity: P3
disposition: deferred
category: docs-contract
pr: 207
reviewed_sha: 628784013580477bd51ca39ea9329161cb2d5c25
location: .github/workflows/pr-policy.yml:53
provenance: introduced_by_feature
first_bad:
guard: deferred: `merge_group` cannot fire until the ruleset carries the queue rule, which is the owner's act after this lands; the first queued entry is the observation
---

## Failure sequence

both workflows declare `merge_group` and pr-policy resolves the entry's pull request from the queue ref -> no entry can exist before the ruleset gains the merge-queue rule -> the resolve step, the second-parent check and the API reads under `GITHUB_TOKEN` are exercised for the first time on the first real entry, after this change has merged

## What the change that takes this up should do

after the ruleset change, watch the first queued entry: confirm `upstroke-pr-policy` ran on the `gh-readonly-queue` ref, resolved the right pull request, and passed; if it fails, the entry leaves the queue and the fix is a follow-up to `pr-policy.yml`; then delete this file

Recorded by PR #207; the validation section of its body says the same.
