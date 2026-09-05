---
id: PR80-MACOS-WORKSPACE-SAMPLER-COLD-PROBE-RECURRENCE
severity: P2
disposition: deferred
category: correctness
pr: 80
reviewed_sha: 2ba66b6e06fa40f9d9fe06dfd21e22517e14d2d6
location: src/workspace_manager.rs
provenance: pre_existing
first_bad: PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE
guard: the project owner — post-promotion sampler hardening
---

## Failure sequence

A recurrence of the `PR7-SAMPLER-SCHEDULES-FROM-A-COLD-PROBE` shape, in a sibling the earlier
fix did not reach. The workspace-manager sampler measures a one-shot probe budget; every scheduled
kill lands after its child has already completed; all 32 observations come back `Completed`; no
killed-child residue is ever sampled, and the required macOS CI leg correctly refuses the vacuous
run. Test:
`workspace_manager::tests::sampled_git_child_kills_every_residue_classified_and_recovered`, hosted
run `33421539013`, macOS job `99584874271`.

**Not attributable to the candidate's source tree**: the candidate's `src/` is byte-identical to
the green `50ed8c86ec60164011bfd393066c4c3696d3865b` source tree, so this is evidence, not a code
regression from that slice.

## What the change that takes this up should do

Give this sampler the discipline already applied once to its sibling — warm-up probe
discarded, median-of-three, recalibration against actual duration, bounded retry — and then
demonstrate on a controlled macOS repetition that a kill actually lands, without masking the
vacuity oracle that caught this. The oracle is doing its job; the schedule is what is wrong.

Recorded in `reviews/FINDINGS.md` §40. The row carries **P2** in the ledger's own words.
