---
id: CI-CFG-UNSHIPPED-UNIX-REGION
severity: P3
disposition: deferred
category: portability
pr: 
reviewed_sha: 
location: .github/workflows/ci.yml
provenance: pre_existing
first_bad: 
guard: the project owner — a platform-scope decision
---

## Failure sequence

A `cfg`-gated Unix region exists that no CI platform compiles, so nothing in the matrix ever
type-checks it. The census already requires the exact acknowledged platform set and fails if that
set changes, so the region cannot grow silently — but it is still code that ships unbuilt on the
runners this project has.

## What the change that takes this up should do

Decide the platform scope: either add a runner that compiles the region, or state that the
region is out of the supported set and let the census pin that statement. This is an owner decision
about what the project supports rather than a repair an implementer can choose.

Recorded in `reviews/FINDINGS.md` §27. Severity is this migration's judgement.
