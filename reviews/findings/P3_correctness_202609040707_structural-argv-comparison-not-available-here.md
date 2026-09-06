---
id: PR136-P3-STRUCTURAL-ARGV-COMPARISON-NOT-AVAILABLE-HERE
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha: 966775e2c9ba0d1a7f317c15451c119e346b4150
location: src/workspace_manager/tests.rs:8456
provenance: pre_existing
first_bad: PR136-P3-SAMPLED-ARGV-CENSUS-IS-FAIL-OPEN
guard: deferred to src/workspace_manager.rs, queue row 11: the honest form compares what the funnel passes with what sampled_command passes as values at…
---

## Failure sequence

the repair above counts literals, which is still a census over source text -> a fixed Git argument spelt as a `const`, a variable or a call moves no count, exactly as `.into()` moved none before -> the sampled child can still diverge from the funnel's real child, and no amount of text matching closes that, because a sibling spelling always satisfies a text census

## What the change that takes this up should do

deferred to `src/workspace_manager.rs`, queue row 11: the honest form compares what the funnel passes with what `sampled_command` passes as **values at run time**, which needs the parent's argv construction to be observable — a seam in the parent, or an argv on the hook phase, and the latter is `src/topology/effects.rs`'s frozen type. Reproduction to use: argv.push("--ignore-errors".into()) in `candidate_stage`, which passes the census at 966775e and is caught only by the literal count added here

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
