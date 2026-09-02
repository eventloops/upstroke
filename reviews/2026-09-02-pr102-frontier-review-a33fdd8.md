# PR #102 — extract the host and container-exec test modules: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, recorded as written; all four findings repaired above the reviewed head |
| **Reviewed SHA** | `a33fdd835298e9fcbd81bf1e11e239884a40d20d` |
| **Merged SHA** | the head this record accompanies: the reviewed head plus the four repairs and a clean master merge-in |
| Pull request | eventloops/upstroke#102, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox, inputs placed in a worktree at the exact head |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree; out-of-scope observations recorded and carried, not repaired here |

## The verdict, verbatim

> ## Findings
>
> 1. **High: the extraction weakens lint enforcement.** The new child repeats the parent’s allow, then suppresses `clippy::duplicated_attributes` for the entire module at [exec/tests.rs:17](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:17) and [exec/tests.rs:30](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:30). Before this change, there was only the outer attribute at [exec.rs:1452](/srv/worktrees/review-102/src/runner/container/exec.rs:1452), with no such suppression.
>
>    Concrete failure: add the same lint or `cfg` attribute twice to a test item. `cargo clippy -- -D warnings` now stays green because the file-wide allow suppresses the warning. Before this PR, it failed. The body admits this residual risk at [pr.md:452](/srv/worktrees/review-102/pr.md:452), but that contradicts its stronger claim that “nothing is widened” at [pr.md:217](/srv/worktrees/review-102/pr.md:217). This is disclosed, so it is not a *silent* scope widening, but it is still a regression introduced by the change. Removing the obsolete outer allow, updating its allowlist row, and removing this suppression belongs in this extraction.
>
> 2. **The new allowlist evidence makes a false containment claim.** The new `exec/tests.rs` row says the file “performs no effect outside a scratch directory” at [effects/allowlist.toml:315](/srv/worktrees/review-102/effects/allowlist.toml:315). The file creates containers in the shared Docker daemon at [exec/tests.rs:4797](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:4797), queries that daemon, and reads the host process table through `/proc` at [exec/tests.rs:5400](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:5400).
>
>    Concrete sequence: a real-Docker test creates its daemon container, then the test process is killed or residue discovery fails. The cleanup guard cannot remove the container, and deleting the scratch directory does not remove daemon state. These effects predate the move, but the inaccurate review assertion is newly added by this diff and must describe them honestly.
>
> 3. **The PR body overstates the gated-test control.** It says the repointed source read proves each listed name “is a test” at [pr.md:70](/srv/worktrees/review-102/pr.md:70). The assertion only searches for `fn {name}(` at [exec/tests.rs:5565](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:5565); the tree-wide counterpart does the same at [container/tests.rs:3594](/srv/worktrees/review-102/src/runner/container/tests.rs:3594).
>
>    Concrete sequence: remove `#[test]` from a `real_docker_*` function while leaving its function and `docker_gate` call intact. Both source censuses pass, but Cargo stops running it. The historical before/after test-list comparison supports this particular move, but the committed control does not prove the claim attributed to it.
>
> 4. **The extraction directly falsifies a census comment.** [container/tests.rs:3082](/srv/worktrees/review-102/src/runner/container/tests.rs:3082) still says `src/runner/host.rs` “has no directory today,” while this diff creates `src/runner/host/tests.rs`. The body itself calls this extraction “that day.” This is incomplete bookkeeping caused by the change.
>
> The production-region hashes, moved test counts, module paths, rustfmt-normalized bodies, seven `include_str!` rebases, self-source path rebase, registry entries, and four explicitly repaired censuses checked out. I found no new production `unwrap`/`expect`, `anyhow`, visibility widening, decision-file edit, or non-`std::path` path construction.
>
> ## Out of scope
>
> - [exec/tests.rs:2285](/srv/worktrees/review-102/src/runner/container/exec/tests.rs:2285) claims to enumerate every Rust file in the container subtree, but its immediate `read_dir` ignores nested directories. A later production `mod volume_admin;` in `exec.rs`, backed by `exec/volume_admin.rs` containing `docker volume prune`, would evade the census. This weakness is unchanged from the base, where existing `census/tests.rs` and `resolve/tests.rs` were already ignored. The newly moved `exec/tests.rs` is test-only and correctly outside the production domain, so this PR does not activate the production false negative.
>
> - The gated-test source scanners’ inability to verify `#[test]` is also pre-existing: the body moved unchanged except for its source path. Only the PR body’s newly asserted stronger claim is in scope above.
>
> VERDICT: CHANGES_REQUIRED
## Disposition

All four findings repaired. The two that needed code:

**Finding 1 — the extraction weakened lint enforcement.** The child repeated
the parent's allow and additionally suppressed `clippy::duplicated_attributes`
file-wide, which did not exist before, so a doubled attribute would have passed
Clippy where it previously failed. Repaired at the cause rather than the
symptom: the now-obsolete outer allow on `exec.rs`'s declaration is deleted,
so stating the level in the child no longer allows one lint twice for one
module -- which is what that lint is -- and the suppression is gone.
`exec.rs`'s allowlist row records the empty `allows` that leaves.

**Finding 2 — the new allowlist row made a false containment claim.** It said
the file performs no effect outside a scratch directory of its own making,
while it creates containers in the shared Docker daemon and reads the host
process table through `/proc`. Those effects predate the move; the sentence
asserting otherwise was new in this diff, so it was this change's to make true,
and it now describes the daemon-visible state and what happens when the
residue guard cannot run.

Findings 3 and 4 narrowed an overstated claim about what the gated-test source
control proves, and updated the census comment this extraction falsified -- the
one saying `src/runner/host.rs` has no directory today.

## Out of scope, carried not repaired

The reviewer recorded two pre-existing weaknesses it judged unactivated by this
diff: the container-subtree census reads only its immediate directory, so a
future nested production module would evade it; and the gated-test source
scanners cannot verify a `#[test]` attribute. Both are carried under the
owner's scoping direction rather than repaired here.
