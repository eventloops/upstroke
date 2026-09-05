---
id: PR163-PASS1-02
severity: P2
disposition: deferred
category: security-trust
pr: 163
reviewed_sha: 2efe6383c46221786d6dab28984a60e89e46ea15
location: .github/scripts/test-internals-notes.sh:100
provenance: pre_existing
first_bad:
guard: "PR156-SHARED-N3, the shared backlink-gate repair in PR #156"
---

## Failure sequence

Replace the runtime note's `[module](../../../../src/runner/container/runtime.rs)`
backlink with plain text `module (../../../../src/runner/container/runtime.rs)`.
The gate still extracts the parenthesized path and resolves it to the expected
source file, so the full gate passes even though the note has no clickable
source link. This contradicts the backlink contract in docs/internals/README.md.

The [SHA-bound prior review](https://github.com/eventloops/upstroke/pull/163#issuecomment-5548851588)
reported this reproduction on the reviewed head. The gate comes from the merged
#144 pilot; PR #163's family migration does not change its logic. P2 is the
implementer's triage of that unlabelled finding. The owner-directed docs policy
records this unresolved finding while the shared repair proceeds in PR #156.
Assignment to that carrier does not establish that the defect is fixed here.

## What the change that takes this up should do

Keep the shared repair in PR #156 under PR156-SHARED-N3. Require a Markdown
source link whose destination resolves to the paired Rust module, and exercise
the full gate with both a valid nested link and the plain-parenthesized-path
reproduction. Preserve existing wrong-module and missing-destination checks.

Integrate and verify the reviewed carrier before marking this finding fixed
and deleting this file. The carrier's executable gate/test changes retain the
non-documentation P2 repair requirement. Do not duplicate the gate repair in
this documentation family or rewrite the closed historical finding ledger.
