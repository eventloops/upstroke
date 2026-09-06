---
id: SWEEP-HOST-ENVIRONMENT-001
severity: P3
disposition: deferred     # deferred | accepted-risk — why it is still here
category: correctness
pr: 181
reviewed_sha: d15d9310dec6ce6c10931ee1c8465a555e27e8d4
location: src/runner/host/environment.rs:45
provenance: pre_existing   # pre_existing | introduced_by_feature | fix_regression | undetermined
first_bad:                # predates the #111 split (6a969a33); exact origin not derived
guard: a follow-up commit to src/runner/host/environment.rs, reviewed under the standard two-review sweep loop
---

## Failure sequence

`HostEnvironment` (`src/runner/host/environment.rs:45`) publicly derives `Clone`, so any caller
can duplicate the whole environment `Vec` and every `OsString` it holds. No call site in the tree
clones a `HostEnvironment` value today (`HostRunner`, its only field owner, does not derive
`Clone` and never invokes `.clone()` on its `environment` field), and neither the source nor
`docs/internals/runner/host/environment.md` states the multi-owner or transfer semantics §6
requires beside a non-trivial clone. Once the sweep marks the file conformant to §6 in full, an
unexplained, unused `Clone` derive on a struct larger than a handle is a concrete instance of the
standard's own text: "A clone of anything larger than a handle is visible at a boundary or
explained beside the call."

## What the change that takes this up should do

Either remove `Clone` from `HostEnvironment`'s derive list (confirming no in-tree or planned
caller needs it), or add a comment beside the derive naming the concrete owned-snapshot or
cross-task-transfer semantics that require duplicating the full environment. Whichever is chosen,
correct this file's `standards/SWEEP.md` row to state it.
