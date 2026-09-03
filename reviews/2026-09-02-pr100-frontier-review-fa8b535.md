# PR #100 — extract the workspace_manager and rundir test modules: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, recorded as written; all four findings repaired above the reviewed head |
| **Reviewed SHA** | `fa8b53500056239b283ff8ef402050aeedceef3d` |
| Pull request | eventloops/upstroke#100, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree |

## The verdict, verbatim

> Changes are required at `fa8b53500056239b283ff8ef402050aeedceef3d`.
>
> ## Findings
>
> 1. The extraction activates an unguarded lint-allow inheritance hole.
>
>    Both parents allow all three governed lints, while none of the four new children states its own lint level. The children already contain denied operations such as `Command::output`, filesystem writes, and recursive deletion ([fixture.rs](/srv/worktrees/w1-funnels/src/workspace_manager/fixture.rs:20), [scratch_tree.rs](/srv/worktrees/w1-funnels/src/rundir/scratch_tree.rs:178)).
>
>    The existing census only covers three unrelated parents ([runner/container/tests.rs](/srv/worktrees/w1-funnels/src/runner/container/tests.rs:3082)); the allowlist only names `src/workspace_manager.rs` and `src/rundir.rs`. Therefore:
>
>    1. Rust inherits the parents’ allowances into all four children.
>    2. Clippy accepts the denied operations.
>    3. The allowlist does not identify the files containing them.
>    4. The census remains green because neither parent is in `FUNNELS`.
>
>    This is exactly a pre-existing guard weakness activated by this diff, making it in scope under the owner’s direction. Reporting it in the body does not make it an independent follow-up. The now-false explanation that `fixture` is “nested inside this file” remains at [workspace_manager.rs](/srv/worktrees/w1-funnels/src/workspace_manager.rs:3752).
>
> 2. The whole-file-module skip control is weakened, contrary to the PR body.
>
>    The suffix control at [recover/tests.rs](/srv/worktrees/w1-funnels/src/engine/topology/recover/tests.rs:7735) does not name `fixture.rs` or `scratch_tree.rs`. The body claims the cardinality check necessarily fails if either is missed, but it uses `>= 22`; a count cannot detect substitution—the exact weakness explained at [cfg.rs](/srv/worktrees/w1-funnels/src/effects/tests/cfg.rs:1197).
>
>    Concrete failure sequence:
>
>    1. A regression in this local filtering leaves `workspace_manager/fixture.rs` in `sources`.
>    2. It mistakenly skips one unrelated production file instead.
>    3. `test_files_skipped` remains 22.
>    4. Every listed suffix still passes because `fixture.rs` is absent from the list.
>    5. A recovery-call needle in that fixture can then satisfy the production-caller census after the real production call is removed.
>
>    The separate registry test does not inspect this local filtering, so it does not close that sequence.
>
> 3. The extraction falsifies more live source documentation than the body discloses.
>
>    Besides the five sites the body lists, the old exhaustive four-file population remains asserted at:
>
>    - [effects.rs](/srv/worktrees/w1-funnels/src/effects.rs:1326) and [effects.rs](/srv/worktrees/w1-funnels/src/effects.rs:1903)
>    - [cfg.rs](/srv/worktrees/w1-funnels/src/effects/tests/cfg.rs:1200) and [cfg.rs](/srv/worktrees/w1-funnels/src/effects/tests/cfg.rs:1212)
>    - [classification.rs](/srv/worktrees/w1-funnels/src/effects/tests/classification.rs:40)
>
>    The diff activates these defects by increasing the non-`tests.rs` population from four to six. Calling them “outside this packet” conflicts with the supplied scope rule and with the summary’s “changes nothing else.”
>
> 4. Several exact PR-body claims are demonstrably false.
>
>    - [pr.md](/tmp/tmp.aODboAvUVr/pr.md:12) says `workspace_manager/tests.rs` has 5,930 lines; the exact-head file and diff contain 5,929.
>    - The commands quoted at [pr.md](/tmp/tmp.aODboAvUVr/pr.md:223) return 22 and 6 hits as written, not 20 and 5. The lower numbers result only after manually excluding references in the moved modules.
>    - [pr.md](/tmp/tmp.aODboAvUVr/pr.md:293) says the extraction creates three children; it creates four.
>    - The rollback claim at [pr.md](/tmp/tmp.aODboAvUVr/pr.md:285) is not operationally correct. Reverting either extraction commit alone leaves its two paths in `WHOLE_FILE_TEST_MODULES`. Reverting the combined registration commit as well removes the other extraction’s entries. Either route breaks the exact registry test unless the registration change is split manually.
>
> ## Out of scope
>
> - [effects.rs](/srv/worktrees/w1-funnels/src/effects.rs:1913) documents that the module-declaration scanner ignores `cfg_attr(..., cfg(test))`. Such a declaration can make a whole test file appear to source censuses as production. This predates the PR, and the new declarations use direct `#[cfg(test)]`, so the change does not activate it.
>
> - The predictable scratch helpers at [rundir/tests.rs](/srv/worktrees/w1-funnels/src/rundir/tests.rs:7) and [workspace_manager/fixture.rs](/srv/worktrees/w1-funnels/src/workspace_manager/fixture.rs:14) recursively pre-clean tag/PID-based paths. PID reuse can cause a later test run to delete an occupied stale path it did not create. These bodies existed identically inline before the extraction; only their file location changes.
>
> VERDICT: CHANGES_REQUIRED
## Disposition

Four findings, four repair commits, one each.

**The first is the one worth reading twice.** The four extracted children
inherited their parents' file-level lint allows while containing denied
operations, and no census objected: the guard that catches exactly this walks
only the three funnels named in `FUNNELS`, and neither `workspace_manager.rs`
nor `rundir.rs` is among them. PR #102 made the same mistake in the same wave
and was stopped by that census, because `host.rs` **is** in the list. Same
defect, one caught by a machine and one only by review.

The reviewer classified it as a pre-existing weakness this diff activates, and
so in scope under the owner's direction. That is the right reading: the hole
predates the change, and the change is what put files with denied operations
into it. Each child now states its own level, with an allowlist row naming the
file and recording what it needs each allow for, measured.

The rest: the whole-file-module skip control now names the two children whose
stems are not `tests`, so a substitution fails by name rather than relying on
a count that cannot see it; the non-`tests.rs` population is stated as six
where five live sites still said four; and the handle surface of the extracted
children is stated exactly.

## Out of scope, carried not repaired

The reviewer recorded the `cfg_attr(..., cfg(test))` blind spot in the
declaration scanner, and the predictable scratch-path helpers whose PID-based
pre-clean can delete an occupied stale path. Both predate this change; the
second moved file location only. Carried under the owner's scoping direction.
