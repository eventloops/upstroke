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
guard: Verify the actual PR156 carrier's ancestry and inspect the host notes' repository and GitHub source links before marking the finding fixed.
---

## Failure sequence

A reader opens the inherited host notes on the published site, whose source
is docs/, then follows the relative source link. It resolves outside that
publishing tree to /src/runner/host.rs, which the site does not publish.

The original PR160 review included this inherited file among seven affected
notes. PR160 repaired its own six family notes. The seventh belongs to the
shared PR156 repair, as agreed by the stewards. Assignment does not resolve
the finding. The owner's docs-only policy permits recording this unresolved
P2 without holding PR160 for its repair; required CI and independent review
remain mandatory.

## What the change that takes this up should do

PR156 owns the host notes repair. Keep a visibly usable repository-relative
link and add a Source on GitHub link to
https://github.com/eventloops/upstroke/blob/master/src/runner/host.rs, with
the supported context of each link stated accurately.

When astra_merge integrates the reviewed carrier from master, verify its
ancestry and both links in PR160's final head. If that resolves this finding,
delete this file and mark PR160-PAGES-HOST fixed in the PR body's permanent
ledger. Until then, retain the deferred record honestly.
