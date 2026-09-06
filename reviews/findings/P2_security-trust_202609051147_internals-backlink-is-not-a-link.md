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

## Independent review addendum, 2026-09-05

The original record above remains unchanged. The final Astra/max review corroborates this open finding at the integrated head:

```yaml
id: PR163-PASS1-02
severity: P2
disposition: deferred
category: security-trust
pr: 163
reviewed_sha: f346f28e6be23c5f9bd3f8decfa64e025dea5c91
location: .github/scripts/test-internals-notes.sh:102
provenance: pre_existing
first_bad: undetermined; present in declared base 9d8418644254cceffe952cbbf73a9e1d6e3fea24
guard: Future N3 gate maintenance should require an actual Markdown source link and include a plain-parenthesized-path negative control.
```

This is a current corroboration draft for the existing PR163-PASS1-02 record, not a new stable ID. The tracked record and earlier reviewed identity remain historical evidence. Append the current evidence when updating that record and ledger; do not create another record for the same ID or rewrite an earlier verdict.

## Failure sequence

Replace the opening Markdown source link in runtime.md with plain prose containing only (../../../../src/runner/container/runtime.rs). The N3 expression at line 102 matches the parenthesized path without checking for a Markdown label. The realpath comparison at lines 107 through 109 then resolves that path to the expected module. The gate can accept a note that has no source hyperlink.

I independently inspected the current matcher and its consumers. The gate blob is identical between the declared base and the reviewed head. No new mutation was run for this inherited finding, and a green CI execution over the existing valid backlinks does not disprove it. Assignment elsewhere is not evidence of repair.

The demonstrated consequence remains missing internal navigation and false assurance from N3. It does not demonstrate a product security regression. The existing P2 severity and security-trust category are preserved. The owner's documentation and limited validation stop rules authorize deferral without another repair cycle. The existing record and canonical row remain required.

## What the change that takes this up should do

Recognize a Markdown link to the source module before resolving its path. Exercise a valid link, a parenthesized path without a link, and an incorrect source destination. Preserve the current recorded disposition until that change is independently verified.
