---
id: PR130-REVIEW3-REPLACEMENT-ISOLATION-STOPS-AT-THE-MANAGER
severity: P1
disposition: deferred
category: correctness
pr: 130
reviewed_sha: 8c5dbc37b0cf3ed2ef28c6a6447368f10c1b0adf
location: src/workspace_manager.rs:2725
provenance: introduced_by_feature
first_bad: 8a2b0bd
guard: deferred, and the claims narrowed at 3e00568752 to exactly what the variable covers: the module doc, the builder's doc, the payload census row,…
---

## Failure sequence

`GIT_NO_REPLACE_OBJECTS=1` is set on the Git children `WorkspaceManager::command` spawns and nowhere else -> a gate or a reviewer running inside the snapshot gets the environment the host runner composes, which clears the ambient one (`src/runner/host.rs`), and `read_only_git`, which `quiescence` uses, is outside that builder -> with `refs/replace/A -> B` installed and the snapshot's filesystem correctly holding `A`, a role process running `git show HEAD:f` in it reads `B` and `git status --porcelain` reports `M f` against an otherwise clean snapshot (reproduced by the reviewer on git 2.43); the regression test asserts filesystem bytes and the raw commit and never runs a role process, so it cannot see this

## What the change that takes this up should do

deferred, and the claims narrowed at 3e00568752 to exactly what the variable covers: the module doc, the builder's doc, the payload census row, `standards/SWEEP.md` and the pull-request body now say that the snapshot's filesystem is the judged tree while a process inspecting it through Git may not see that tree, and no sentence anywhere says the engine never reads a replacement. **The proposal**, needing an owner design ruling before it is written: set the variable in the host runner's environment composition and in `read_only_git`, and add a sentence to `design/` saying whether an exact snapshot is defined against raw objects or against replacement objects, since `DESIGN.md`'s "ground truth is the diff" does not say and the behaviour is product-wide for every repository with rewritten history. Guard until then: the narrowed docs, and `a_snapshot_ignores_a_replacement_object_and_materialises_the_judged_tree`, which pins the half that does hold

Recorded by the PR #130 `src/workspace_manager/snapshot_ref.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
