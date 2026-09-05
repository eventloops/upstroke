---
id: SWEEP-WORKTREE-013
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: ea4fd748236917e10eab916c3266d2ce976672f6
location: src/workspace_manager/worktree.rs:226
provenance: pre_existing
first_bad: —
guard: deferred to the parent's sweep, queue row 11, with the proposal: the add funnel (WORKTREE_ADD_ARGV, worktree add --detach --quiet, no lock of its…
---

## Failure sequence

`is_initializing` compares the lock reason with Git's word and the doc at the reviewed head called it an invariant -> `git worktree lock --reason initializing <path>` on a populated worktree lists `locked initializing`, the helper answers `true`, `quiescence` answers `Unpopulated`, and an open generation is removed with force and re-added while a retained one is closed -> a user-controlled reason read as provenance; the behaviour is the base's (`workspace_manager.rs:1601` at `0bff83d` compared the same string), and the reviewed head certified it (pass-1 finding 1)

## What the change that takes this up should do

deferred to the parent's sweep, queue row 11, with the proposal: the add funnel (`WORKTREE_ADD_ARGV`, `worktree add --detach --quiet`, no lock of its own) passes `--lock --reason` with a token only the engine writes (its prefix, the run id and the slot), so a populated engine worktree is always locked with the engine's token, `git worktree lock` refuses a second lock, and a reason that is neither the token nor Git's `initializing` is a distinct refusal; that reaches forced removal (`--force --force` for a locked worktree), `worktree prune`'s skip of locked entries and the residue classifier's after-reference, which is why it is the parent's. At 5e19848d3eae6b16d96c2786eaf708282aba7fa1 the doc says what is true: the record cannot tell Git's word from a writer's, both read `true`, and a writer to the execution root who forges it is inside the trust boundary the root assumes (§14, the same writer can delete the checkout). `a_bare_lock_is_a_lock_without_a_reason_and_only_initializing_is_initializing` pins the documented reading; the token's two-case test (bare word not read as initializing, the token read as such) belongs to row 11 with the funnel

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
