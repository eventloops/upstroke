---
id: PR136-PASS3-P1-CENSUS-EXTENT-FORGED-BY-A-DECOY
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager/tests.rs:8718
provenance: introduced_by_feature
first_bad: PR136-PASS2-P1-CENSUS-EXTENT-FORGED-BY-A-LITERAL
guard: withdrawn rather than repaired a third time, on the coordinator's narrowing ruling of 2026-09-04: a census that reads as a guarantee and can be…
---

## Failure sequence

`sampled_argv_counts` takes the **first global** `.find(signature)` and never proves the match lies in `impl WorkspaceManager` -> an earlier method of the same name with an empty body on another type absorbs the scan, and both end scans agree on that decoy so the withdrawn boundary check stayed false -> the real `candidate_stage` takes `argv.push("--ignore-errors".into())`, the census reports 0/0/0, every control passes, and the sampled child runs an argv the funnel does not. This is the **second** forgery of this extent: pass 1 forged it with a comment, pass 2 rebuilt it on blanked text, pass 3 forged it at the function boundary

## What the change that takes this up should do

**withdrawn rather than repaired a third time**, on the coordinator's narrowing ruling of 2026-09-04: a census that reads as a guarantee and can be walked around silently is worse than none, because the green is what gets trusted. The false-boundary field and its assertion are deleted with the naive-extent computation that fed them; the structural extent is kept because reverting it restores pass 2's literal forgery. What `no_sampled_funnel_builds_its_argv_from_a_literal` now claims, in its own doc and in `CANDIDATE_STAGE_ARGV`'s, is that these four bodies hold no inline literal Git argument today — a tripwire, not a guarantee — and **§12's domain-boundary requirement is recorded here as unmet**. A real census must resist all three recipes: the sibling spelling, the false boundary, and the decoy function

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
