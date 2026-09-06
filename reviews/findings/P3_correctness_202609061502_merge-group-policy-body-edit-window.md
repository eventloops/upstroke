---
id: PR207-MERGE-GROUP-BODY-EDIT-WINDOW
severity: P3
disposition: deferred
category: correctness
pr: 207
reviewed_sha: 628784013580477bd51ca39ea9329161cb2d5c25
location: .github/workflows/pr-policy.yml:53
provenance: introduced_by_feature
first_bad: PR207-ENTRY-HEAD-UNVERIFIED
guard: deferred: the same window exists on today's path between the last pull-request run and the merge click; the merge queue does not re-run an entry on a body edit
---

## Failure sequence

a queue entry's `upstroke-pr-policy` run validates the pull request's live body and passes -> the author edits the body into an invalid state while the entry is still building -> the edit triggers a run on the pull request's own head, not on the entry, and the entry's green context stands, so the queue can land a pull request whose body no longer validates

## What the change that takes this up should do

either re-validate the body at merge time from a workflow that runs on the `pull_request: closed` event and reopens or reverts when the body fails, or accept the window as equal to the pre-queue one and say so in MAINTAINING; the choice is the owner's

Recorded by PR #207 from review pass 1 (finding 5); the entry-head check was the repair, and this is the half of it that cannot be closed in the workflow.
