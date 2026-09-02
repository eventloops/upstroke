# PR #97 — refactor(engine): extract the inline test module of prelock into a sibling file: frontier review record

| field | value |
|---|---|
| **Verdict** | **PASS**, recorded as written |
| **Reviewed SHA** | `fbcf5b313844487afcb9c56dd0a97b0f16130f95` |
| Pull request | eventloops/upstroke#97, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 97` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

## The verdict, verbatim

Blockquoted byte-for-byte from the reviewer's saved output
(`~/review-pr97-fbcf5b3.md` on the review box).

> No in-scope findings.
>
> I verified that:
>
> - The supplied diff exactly matches clean head `fbcf5b313844487afcb9c56dd0a97b0f16130f95` against parent `fbf300a`.
> - The production prefix is byte-identical. The moved test body has only the two disclosed rustfmt reflows and no file-relative constructs.
> - Both parent and head expose 1,848 tests with hash `6fe9f0d53113ecdcf980756c44f206b94b5b0a3652eb2b1696e3e499bf4441c0`.
> - All relocated prelock tests pass: 10 active, one intentionally ignored. The subprocess test still selects exactly one child.
> - The census entry is correct and sorted. The complete 111-test effects/census namespace passes.
> - Clippy passes with all targets/features, confirming the moved panic/expect sites remain test-only.
> - No visibility, production API, `anyhow`, path-handling, or platform-sensitive change was introduced.
>
> I could not construct a concrete harm sequence caused by this change.
>
> ## Out of scope
>
> - [prelock/tests.rs:634](/srv/worktrees/w1-prelock/src/engine/topology/prelock/tests.rs:634) converts `current_exe()` through lossy display text before execution. A non-Unicode executable path can be changed to replacement characters, causing `HostRunner` to spawn the wrong path. This was identical at parent `prelock.rs:908`; the move neither introduced nor activated it.
> - [prelock/tests.rs:316](/srv/worktrees/w1-prelock/src/engine/topology/prelock/tests.rs:316) converts `read_dir` failure to an empty census and silently drops entry errors. Transient failures affecting both snapshots can therefore let a residue regression satisfy equality. This was identical at parent `prelock.rs:590`.
> - [effects.rs:2173](/srv/worktrees/w1-prelock/src/effects.rs:2173) ignores `cfg_attr(..., cfg(test))`. Such a declaration can compile a whole-file test module while remaining absent from the module census, leaving downstream scans to treat it as production. This documented gap predates the PR, and the new declaration uses the correctly detected literal `#[cfg(test)]` form.
>
> VERDICT: PASS