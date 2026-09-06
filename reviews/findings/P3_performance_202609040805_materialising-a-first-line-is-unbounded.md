---
id: SWEEP-CLASSIFY-012
severity: P3
disposition: deferred
category: performance
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir/classify.rs:206
provenance: pre_existing
first_bad: 7a83e69
guard: deferred: the doc on first_line_within now states the property where a reader meets it, and what to bound is what a census may spend rather than…
---

## Failure sequence

the scan is constant memory and the answer is not: a first line that is found is materialised at its own length, because the packet states no size exception and the parse needs the whole line -> a run directory whose `events.jsonl` has a first newline gigabytes in costs the census that many bytes of process memory -> the module documents `SCAN_CHUNK` as the cost of "there is no newline" and said nothing about the cost of finding one; no allocation failure is reachable from any log this project writes

## What the change that takes this up should do

deferred: the doc on `first_line_within` now states the property where a reader meets it, and what to bound is what a census may spend rather than what a first line is, which is a decision for the census and its design sentence rather than a constant invented in this module

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
