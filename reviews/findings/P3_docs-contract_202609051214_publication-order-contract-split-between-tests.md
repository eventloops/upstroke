---
id: ASTRA158-006
severity: P3
disposition: deferred
category: docs-contract
pr: 158
reviewed_sha: 105c9e1509efe6cbfbe6d93e8d930c289056f041
location: docs/internals/engine/topology/create/tests.md:250
provenance: pre_existing
first_bad:
guard: "Read the two note sections against their tests: the publication-order paragraph belongs with the_publication_prefixes_run_in_the_packets_order, and the creation-to-loop explanation belongs with a_created_run_hands_itself_to_the_loop. This documentation finding is nonblocking under the owner's documentation review direction recorded in the PR."
---

## Failure sequence

A reader opens the publication-order test's note section -> its contract consists only of the detached final word committed, while the preceding P0-P8 paragraph interrupts the creation-to-loop section -> the preserved contract is attached to the wrong test and is incomplete in both places.

## What the change that takes this up should do

Reunite the publication-order paragraph under its own test and keep the creation-to-loop explanation under the creation test. Read both sections against the corresponding assertions and preserve the existing contract text.

## Review history and evidence

ASTRA158-006 was independently reported as P3/docs-contract at 105c9e1509efe6cbfbe6d93e8d930c289056f041. The interrupted comment and detached final word pre-existed in the declared base and were migrated by a08ea236ddab3b3d9f9471a126a4203494db0645. The exact first-bad commit is not established.

The note's interrupted paragraph is at lines 250 through 259, and its final word appears at line 283 under the_publication_prefixes_run_in_the_packets_order. The corresponding candidate tests are at src/engine/topology/create/tests.rs lines 793 and 833. The independent report also records the interrupted comment at declared-base source lines 924 through 931 and the detached word at line 988.

[Independent review of 105c9e1509efe6cbfbe6d93e8d930c289056f041](https://github.com/eventloops/upstroke/pull/158#issuecomment-5551707422).
