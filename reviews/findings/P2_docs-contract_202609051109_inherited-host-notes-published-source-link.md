---
id: PR160-PAGES-HOST
severity: P2
disposition: deferred
category: docs-contract
pr: 160
reviewed_sha: d9359fae74bc43a6dbbacb0b3ff5ce8339454f2e
location: docs/internals/runner/host.md:3
provenance: pre_existing
first_bad: b684d565b365f1c83d4a2ebedf8e0a4e04a9cf72
guard: Verify the PR156 carrier is an ancestor of the final PR160 head and host notes retain a usable repository backlink plus a Source on GitHub link to the actual module.
---

## Failure sequence

A reader opens the inherited host notes on the published site, whose source
is docs/, then follows the relative source link. It resolves outside that
publishing tree to /src/runner/host.rs, which the site does not publish.

The original PR160 review included this inherited file among seven affected
notes. PR160 repairs its own six family notes. The seventh belongs to the
shared PR156 repair, as agreed by the stewards. Assignment does not resolve
the finding, and this row is not permission to merge with it unresolved.

## What the change that takes this up should do

PR156 owns the host notes repair. Keep a visibly usable repository-relative
link and add a Source on GitHub link to
https://github.com/eventloops/upstroke/blob/master/src/runner/host.rs, with
the supported context of each link stated accurately.

After astra_merge integrates the reviewed carrier from master, verify its
ancestry and both links in PR160's final head, then delete this file and
mark PR160-PAGES-HOST fixed in the PR body's permanent ledger.
