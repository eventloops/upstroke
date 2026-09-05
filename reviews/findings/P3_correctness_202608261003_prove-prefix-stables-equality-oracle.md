---
id: PR5-EVENTS-020
severity: P3
disposition: deferred
category: correctness
pr: 5
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — the G2 adjudication sitting
---

## Failure sequence

A withheld-mutation-catalogue entry targeting `prove_prefix_stable`'s equality oracle (`src/events/log/tests.rs:1306`, `:2594`, `:2683`). On re-measurement against the shipped
tree it comes back `KILLED`, and the killing assertion is confirmed still present, byte-identical
or near-identical.

## What the change that takes this up should do

Compare the re-expressed patcher against what the entry's prose actually specifies, and
decide which of two things happened: the assertion was **narrowed**, which is a real loss of
detection power — rounds 3 to 6 worked heavily on `events/log.rs` and the residue harness — or the
re-expressed mutation is an **equivalent mutant**, unkillable by construction and a false positive
of re-implementing a mutation that was only ever recorded in prose. Settling it is mechanical and
bounded. Do not carry these forward as "five regressions": that is the claim this exercise exists
to avoid making without evidence.

Recorded in `reviews/FINDINGS.md` §15 as one of the six entries needing adjudication. Severity is this migration's judgement: the entry is not yet classified as a regression.
