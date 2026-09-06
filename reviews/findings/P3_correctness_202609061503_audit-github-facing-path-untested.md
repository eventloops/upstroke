---
id: PR207-AUDIT-API-PATH-UNTESTED
severity: P3
disposition: deferred
category: correctness
pr: 207
reviewed_sha: 628784013580477bd51ca39ea9329161cb2d5c25
location: scripts/pr-ready-audit.sh:292
provenance: introduced_by_feature
first_bad: PR207-AUDIT-UNTESTED
guard: deferred: the pure parts have a fixture gate (test-pr-ready-audit.sh); the GitHub- and git-facing half needs a fake `gh` and a fixture repository, which is a harness of its own
---

## Failure sequence

`audit_one` composes eight API reads and a handful of git operations per pull request -> only the pure helpers it calls are exercised by a gate -> a regression in the composition (the order of the head re-reads, the base check, the state machine) is caught by a reviewer reading, or by a live pull request, not by CI

## What the change that takes this up should do

add a harness that puts a fake `gh` on PATH answering from fixture JSON and runs `audit_one` against a throwaway repository with scripted heads, merges and reviews, one scenario per state the audit can report (READY, NOT-READY per blocker, NEEDS-ATTEST, MANUAL, HEAD-MOVED, BASE-MOVED)

Recorded by PR #207 from review passes 7 and 11.
