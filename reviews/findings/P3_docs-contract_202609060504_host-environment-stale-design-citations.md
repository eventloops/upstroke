---
id: SWEEP-HOST-ENVIRONMENT-002
severity: P3
disposition: deferred     # deferred | accepted-risk — why it is still here
category: docs-contract
pr: 181
reviewed_sha: d15d9310dec6ce6c10931ee1c8465a555e27e8d4
location: src/runner/host/environment.rs:133
provenance: pre_existing   # pre_existing | introduced_by_feature | fix_regression | undetermined
first_bad:                # predates DESIGN.md's split into design/; exact origin not derived
guard: a follow-up commit to src/runner/host/environment.rs and docs/internals/runner/host/environment.md
---

## Failure sequence

`HostEnvironment::preflight`'s refusal message (`src/runner/host/environment.rs:133`) cites
`DESIGN.md:258-264` when an overlay names a reserved key. `DESIGN.md` is now a 99-line index (the
design lives in `design/`, split by section); there is no line 258 to read. The paired notes file
`docs/internals/runner/host/environment.md` repeats the same dead line reference at four more
sites (lines 12, 98, 114, 120) and additionally cites the retired
`decisions/2026-08-12-merge-queue-execution-topology.md` at two of them (lines 120, 156); that
directory no longer exists in the tree. The live content is `design/08_design_trait_surface.md`
lines 57-63 (the role-scoped `HOME`/`PATH`/credential-location paragraph, "Probe and execution
compose the same base, mounts, reserved values, and overlay") and
`design/26_design_merge_queue_protocol.md` lines 388-389 and 398-399 (the gate-shell boundary
sentence and "Host runner behavior remains available and honestly provides no OS boundary around
gate code"), both verified present at those line numbers on this review's base SHA.

## What the change that takes this up should do

Replace the `DESIGN.md:258-264` citation in the `preflight` refusal message with the current
`design/08_design_trait_surface.md` section reference, and repoint the four
`docs/internals/runner/host/environment.md` citations (the two remaining `DESIGN.md:258-264`/
`DESIGN.md:263` references and the two `decisions/2026-08-12-merge-queue-execution-topology.md`
references) to `design/08_design_trait_surface.md` and `design/26_design_merge_queue_protocol.md`
respectively, quoting each replacement passage verbatim before citing it. This is a citation
repoint only; no behavior described by either source changes.
