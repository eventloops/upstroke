---
id: SWEEP-CHECK_CANDIDATE-001
severity: P3
disposition: fixed
category: correctness
pr: 186
reviewed_sha: f301b7b38e60c0df00ba89c0a42fa179bf017c01
location: src/topology/fold/check_candidate.rs:132
provenance: pre_existing
first_bad:
guard: fixed in this pull request by rewriting the match over all four named combinations
---

## Failure sequence

`check_candidate_prepared`'s final match dispatched on `(&prepared.lease_effect, entry.lineage)` —
`CandidateLeaseEffect` (two variants) crossed with `Option<Lineage>` — naming the two matching
combinations and catching the other two with `_ =>`. A third `CandidateLeaseEffect` variant is
added -> the new variant crossed with either `Option` state falls into the wildcard arm instead of
failing to compile -> the fold accepts or refuses the new kind by the old generic "this does the
other one" message, with no reviewed decision about what the new kind actually means at this site.

## What the change that takes this up should do

Fixed in this pull request: the match now names all four combinations, the two "mismatched" ones
combined with an or-pattern instead of `_`. Adding a third `CandidateLeaseEffect` variant will now
fail to compile here (non-exhaustive match) until a reviewer decides what it means at this site.
Behavior is unchanged for the existing two variants: the `WidensLineage`+no-lineage combination is
pinned by `a_candidate_is_prepared_by_the_generation_whose_attempt_is_in_flight` in
`src/topology/fold/tests.rs`, which failed when this arm's body was replaced with a no-op (mutation
witness M11 in the pull request body).
