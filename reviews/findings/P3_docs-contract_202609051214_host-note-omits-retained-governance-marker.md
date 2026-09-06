---
id: ASTRA158-003
severity: P3
disposition: deferred
category: docs-contract
pr: 158
reviewed_sha: 105c9e1509efe6cbfbe6d93e8d930c289056f041
location: docs/internals/runner/host.md:4
provenance: introduced_by_feature
first_bad: 929019ca455f76f95c3b4f7fcedf8f02bbcb1638
guard: "Compare the host-note introduction with the retained source header and disclose the two-line allowlist-placement marker without deleting required source text. This documentation finding is nonblocking under the owner's documentation review direction recorded in the PR."
---

## Failure sequence

A reader relies on the host note's claim that source contains only its pointer and no inline comments -> the source still retains a two-line allowlist-placement comment -> the introduction omits a required governance exception.

## What the change that takes this up should do

Disclose that the allowlist-placement marker remains beside the governed source allowance. Keep the marker at its required source site and check the introduction against that header.

## Review history and evidence

ASTRA158-003 was independently reported as P3/docs-contract at 105c9e1509efe6cbfbe6d93e8d930c289056f041. The false introduction was introduced in prerequisite commit 929019ca455f76f95c3b4f7fcedf8f02bbcb1638 and is inherited relative to this family. The stewards confirmed that it is distinct from R164-ASTRA-04, which concerns source lookup headings.

The false introduction is at docs/internals/runner/host.md lines 4 and 5. src/runner/host.rs lines 3 and 4 retain the allowlist-placement comment. The note's allowance explanation does not say that this marker remains in source, as the shared README requires. The independent source-audit.json preserves the retained comment inventory.

[Independent review of 105c9e1509efe6cbfbe6d93e8d930c289056f041](https://github.com/eventloops/upstroke/pull/158#issuecomment-5551707422).
