---
id: PR146-ASTRA-001
severity: P3
disposition: deferred
category: docs-contract
pr: 146
reviewed_sha: 94e024ebce234cc3e59b476e72f5869cdceb57af
location: reviews/findings/P3_correctness_202609042224_sampling-n-is-the-registrys-own-number.md:41
provenance: introduced_by_feature
first_bad:
guard: correct the sampling deferral's design citation; schedule a future authority gate only through a reviewed design change
---

## Failure sequence

The sampling-authority finding tells a maintainer that DESIGN section 21
puts a registry.json gate in PR10. The maintainer follows the citation to
design/21_design_versioned_scope.md. Its only numbered build-order sentence,
at line 5, is the v0.1 sequence, where step 10 is connect, capacity preview,
dry-run and polish. Neither that section nor DESIGN.md or the other design
sections names PR10 or schedules a registry.json gate. The deferral thus
presents an unscheduled follow-up as assigned design work.

The incorrect statement is at lines 41 and 42 of the cited finding file.
The current section 26 bijection contract records the sampling-authority
limit but establishes no future gate schedule. This is a documentation
reference error, with no demonstrated checker or test failure.

## What the change that takes this up should do

Remove the unsupported schedule claim, cite section 26 for the current
sampling-authority limit, and describe an external budget authority and
cross-run gate as proposed design work. If that work is scheduled, add its
actual schedule to the living design through a reviewed change and cite it.
