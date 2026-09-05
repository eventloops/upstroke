---
id: ASTRA165-008
severity: P3
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: Cargo.toml:23
provenance: pre_existing
first_bad: PR156-SHARED-PACKAGING
guard: PR156-SHARED-PACKAGING must compare the packaging explanation with the manifest exclusions and retained internal notes
---

## Failure sequence

The Cargo comment says internal documentation is outside the published library because it is website material, but the exclusion list removes only `docs/index.html` and `docs/CNAME`. The internal notes remain package payload, so the comment contradicts the manifest and misstates what a crate consumer receives.

## What the change that takes this up should do

Explain that internal notes remain packaged while site assets are excluded, preserving the existing exclusions and the package's actual contents.

The reviewer established the mismatch from the manifest comment at lines 23
through 25 and exclusions at lines 32 and 33. The PR body's package disclosure is
accurate. This finding makes no size claim and proposes no exclusion-policy change.
Prior row PR165-SHARED-004 and carrier PR156-SHARED-PACKAGING refer to this finding.
The carrier assignment remains pending actual integrated repair evidence.
