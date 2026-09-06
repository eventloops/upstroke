---
id: PR156-ASTRA-PROBE-VERSION-COUNT
severity: P3
disposition: deferred
category: docs-contract
pr: 156
reviewed_sha: 917dcacbc92479d5b30a65438fc04677a2e8a770
location: docs/internals/agent/mod.md:554
provenance: pre_existing
first_bad: 11d0a44837bd0e6155799153a0611b62f52018dd
guard: "Compare the documented version count with the two literal fixture versions and versions.len() assertion in two_boundaries_in_one_process_each_certify_their_own_cli."
---

## Failure sequence

A maintainer reads that the boundary-probe test asserts three distinct versions,
then relies on that statement as a description of its coverage. The actual test
uses only 1.1.1 and 2.2.2, reverses their order, and asserts a set size of two.
The note overstates the number of distinct version values exercised. This is a
documentation mismatch; no runtime failure or inadequate two-version test was
demonstrated.

The same mismatch exists in the source comment at the first-bad commit. The
phrase is absent from that commit's parent. Independent evidence is retained in
the reviewer's probe-version-count-917dcacb.json.

## What the change that takes this up should do

Describe two distinct versions in both orders, matching the existing test. Do not
expand the test merely to match stale prose. This lower-severity documentation
finding is deferred under the owner's 2026-09-05 documentation direction.
