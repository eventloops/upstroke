---
id: PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED
severity: P2
disposition: deferred
category: correctness
pr: 136
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager.rs:1868
provenance: pre_existing
first_bad:
guard: the stream the coordinator spawned on 2026-09-05 for `remove_worktree` convergence (PR #151 is its first result); escalates to the owner if a later pass labels it P1 or P2 rather than accepting the deferral
---

## Why this file exists at all

`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` is closed and its file deleted: the sampler kills
the process group now, which is what production's only kill path does, and it waits for that group
to empty before it inspects. **What that closes is one of the two error codes that entry recorded,
and the suite is not thereby green** — `Git { … has an empty gitdir }` is untouched by any kill
shape and has its own file. **Nor is the question that finding left open gone with it.** That entry
wrote
down, in its own last paragraph, what its experiment did *not* establish, and a question does not
stop being open because the entry carrying it was resolved. It is here, alone, so that closing the
first one does not close this one by accident — and so that nobody re-runs the sampler comparison
believing it answers this.

## Related, and what turned out not to be related

The parent entry recorded a second error code under the same assertion — a `git worktree add`
interrupted with a zero-length `gitdir` on disk, which `remove_worktree` refused. On this branch it
was diagnosed as a contract defect (`recovery_proven` promised for a residue the funnel correctly
declined) and filed; **that diagnosis was measured wrong by PR #151**, which fixed it: on git 2.43
`git worktree list` skips a zero-length `gitdir` silently, the classifier answers `None`, and the
repair is a one-condition skip in `revalidate_removal`. That fingerprint is inert residue and is
fixed; this one is a live writer nothing killed, and is open. They share a parent and an assertion
and nothing else. `SAMPLER-SIBLING-KILLS-A-BARE-CHILD` is a third thing again and belongs to the
harness rather than the funnel.

