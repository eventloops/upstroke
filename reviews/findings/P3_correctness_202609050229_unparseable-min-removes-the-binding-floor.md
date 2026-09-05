---
id: SWEEP-ANNOTATION-001
severity: P3
disposition: deferred     # beyond every sweep session's reach: a design decision on refusal
category: correctness
pr: 169
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/plan/markdown/annotation.rs:130
provenance: pre_existing
first_bad:
guard: the warning text pinned by `an_unknown_min_tier_warns_that_no_floor_binds`; the `design/09` paragraph recording the warn-never-error posture for values
---

## Failure sequence

An author floors a task at `mid` and mistypes it: `<!-- upstroke: id=fix-obo min=mdi -->`.
`Tier::parse` answers `None`; `parse_annotation` warns "unknown min tier `mdi` in section
`Fix off-by-one`; ignored, so no floor binds and the task may run at any tier" and leaves
`Annotation::min` absent. `validate` succeeds (warnings never block it), the run's report
carries the same line, and `route` clips nothing because `Task::min_tier` is `None`. The task
the author required to run at `mid` or above runs wherever routing puts it. The one signal is
a warning line; nothing later restores the floor.

The same posture covers an unterminated `<!-- upstroke:` (warned, nothing applied, the
section body swallowed by the block) and a second annotation on one task (the first applies).

## What the change that takes this up should do

Decide, in `design/09`, whether a value the grammar cannot parse on a **binding** attribute
(`min=`) — and an unterminated upstroke comment — should refuse at
`MarkdownPlanAdapter::parse_with_warnings` (an `UpstrokeError::Parse` naming the section and
the value) rather than warn. The design's stated posture is "Unknown attributes warn, never
error"; PR #169 wrote what the code does for values beside it. Refusing is a change to that
sentence and to `validate`'s observable behaviour, which is why it is the owner's. A later pass
labelling this P1 or P2 escalates to the owner rather than re-deferring.
