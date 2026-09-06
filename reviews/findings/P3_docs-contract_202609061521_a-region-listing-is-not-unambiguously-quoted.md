---
id: SWEEP-REGION-002
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/region.rs:5
provenance: pre_existing
first_bad:
guard: a change that gives `describe_region` an escaping quote, or the successor that types
  `FoldError`'s rendered fields (`SWEEP-REGION-001`)
---

## Failure sequence

`describe_region` renders a prefix list by wrapping each path in backticks and joining with
`", "`, and a `GitPath` is an unvalidated `String`: it comes from a plan's `path_hints` (author
text, normalized by `predicted_region` but never restricted in alphabet) or straight off the event
log, where `PathSet` deserializes with no check on the bytes of a path.

A single path whose text is a backtick, a comma, a space and a backtick between two names therefore
renders exactly as the two-path region of those two names does. The one caller,
`check_attempt.rs`'s predicted-region comparison, prints both sides of a refusal that fired
*because* the two path sets are unequal, so the sentence reads as a contradiction: it reports the
region it took and the region the entry's hints derive as the same list. Nothing decides on the
string, so the event is still refused and the fold's behaviour is right; what is lost is the
reader's ability to see why.

## What the change that takes this up should do

Render each path through an escaping quote rather than a bare backtick pair — the `{:?}` of the
`str`, or a helper that escapes a backtick — so that one rendering has one region behind it, and
pin it with a path containing a backtick. Whoever takes `SWEEP-REGION-001` will be in this
sentence anyway. Left as it stands by the row 37 sweep: the refusal identifies its operation and
its task key, which is what CODING_STANDARDS §13 requires of a diagnostic, and changing every
region rendering in the fold to an escaped form is a wider decision than that sweep's bound.
