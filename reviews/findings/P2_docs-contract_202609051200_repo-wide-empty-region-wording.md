---
id: ASTRA165-004
severity: P2
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: docs/internals/topology/paths.md:96
provenance: pre_existing
first_bad: —
guard: Align the paths contract with the RepoWide and empty-prefix case in regions_overlap_component_wise_and_repo_wide_overlaps_everything
---

## Failure sequence

The paths notes say an empty region overlaps nobody, but `PathSet::Prefixes { paths: vec![] }` and `RepoWide` overlap through `src/topology/leases.rs:38`. The named regression test passes, so the note contradicts the actual distinction between an empty bounded region and an unbounded repository-wide region.

## What the change that takes this up should do

State that an empty prefix set overlaps no bounded region while `RepoWide` overlaps it, and keep the wording aligned with the implementation and regression test.

The case is asserted at `src/topology/fold/tests.rs:7721`. The independent review
retains Ubuntu and Windows CI logs showing the named test passing on the reviewed
tree. `docs/internals/topology/leases.md:61` already states the exception. Preserve
the conservative implementation result when correcting the contradictory note.
