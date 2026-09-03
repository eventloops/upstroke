# PR #98 — refactor(topology): extract the inline test modules of fold and effects into sibling files: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, recorded as written |
| **Reviewed SHA** | `27d945f31972ab0e9e68a599c7d4e8dbfd43c196` |
| Pull request | eventloops/upstroke#98, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 98` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

## The verdict, verbatim

Blockquoted byte-for-byte from the reviewer's saved output
(`~/review-pr98-27d945f.md` on the review box).

> ## Findings
>
> 1. [pr.md:232](/srv/worktrees/review-98/pr.md:232) gives incorrect rollback instructions. Commit `27d945f` alone added the two census entries; neither extraction commit touches `src/effects/tests/cfg.rs`. Sequence: revert `4cae798` and `cc594e9` -> both child files and declarations disappear -> the two list entries remain -> the resolver finds 18 modules while `WHOLE_FILE_TEST_MODULES` still expects 20 -> `the_whole_file_test_modules_are_resolved_from_the_declarations_not_the_file_names` fails. A complete rollback must also revert `27d945f`.
>
> 2. [pr.md:134](/srv/worktrees/review-98/pr.md:134) claims there are two cross-file doc-comment references. There are four: [run.rs:1981](/srv/worktrees/review-98/src/engine/topology/run.rs:1981) and `recover/tests.rs` at lines 5201, 6355, and 6828. There are two unique referenced test names, and both still resolve, but the stated census count is unsupported as written.
>
> I found no code-level regression or project-rule violation. The production hashes, test names, rustfmt-only transformations, sorted path registration, full test suite, formatting check, and all-target Clippy check passed at the exact head.
>
> ## Out of scope
>
> - [source_oracles.rs:151](/srv/worktrees/review-98/src/effects/tests/source_oracles.rs:151) does not exclude whole-file test modules from the topology funnel census. This defect predates the change. Although the walk now visits the two new children, neither contains any of the six needles, so no false positive is activated here. Concrete separate failure: extract `registry.rs` tests -> `rundir::create_public_dir` moves into `registry/tests.rs` -> `production_region` treats the whole child as production -> the census falsely reports a production funnel call.
>
> VERDICT: CHANGES_REQUIRED