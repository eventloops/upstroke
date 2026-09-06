---
id: PR166-INTEGRATION-NOTES-NAVIGATION
severity: P3
disposition: deferred
category: docs-contract
pr: 166
reviewed_sha: fb46ebec8297099dec72ae6655d662f2d7975758
location: docs/internals/util.md:162
provenance: fix_regression
first_bad: 3d0bea8b247664bbabc2c99c14715e72e05c464d
guard: Restore scoped, source-searchable headings and remove empty duplicate sections in the affected notes. Preserve the repaired behavior descriptions. Check against docs/internals/README.md lines 54-55 and 76-83.
---

## Failure sequence

The PR #166 merge regenerated whole notes files with bare source fragments instead of enclosing-item headings. Six `#[must_use]` headings in util.md and more than twenty `#[serde(default)]` headings in events/mod.md become indistinguishable. Eight empty duplicate sections also appear in status.md, engine/report.md and events/mod.md. Readers cannot tell which item a contract belongs to. The internals-notes gate checks pointers and backlinks, so its pass does not establish navigation quality.

## What the change that takes this up should do

Restore scoped, source-searchable headings and remove empty duplicate sections in the affected notes. Preserve the repaired behavior descriptions. Check against docs/internals/README.md lines 54-55 and 76-83.

## Review and disposition

The [gpt-5.6-sol max review of PR #166](https://github.com/eventloops/upstroke/pull/166#issuecomment-5555195504) reported this as a low-impact documentation finding without a numeric severity. P3 is delivery triage, not a reviewer-assigned label. The owner merged #166 and requested these two documentation findings be recorded on #172 after master integration. This record is unresolved and claims no repair. The original review verdict remains CHANGES_REQUIRED.
