## 1. Authority and scope

These standards govern implementation quality for all Rust in this repository: production code,
tests, examples and build support. `DESIGN.md` governs product behaviour and architecture and wins
a conflict; `MAINTAINING.md` governs how a change lands. Reconcile a conflict in the same change
rather than choosing silently.

**MUST** and **MUST NOT** are requirements: a deviation needs a reviewed change to the standard,
not an ad hoc exception. **SHOULD** is the default: deviate with a stated reason in the code or the
pull request. Everything else is guidance.

A change leaves the code it materially touches compliant. Existing code is not precedent against a
standard, and unrelated debt is recorded rather than fixed inside an unreviewable scope expansion.
Some standards are newer than the tree; `standards/SWEEP.md` says which, and how existing code is
being brought up to them.

A rule is automated only where a named mechanism examines the code, and each standard ends by
naming its mechanism. Everything else is review. A green gate is not evidence for a review-only
rule, and a missing mechanism never means compliant.
