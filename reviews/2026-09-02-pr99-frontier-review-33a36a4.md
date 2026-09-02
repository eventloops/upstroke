# PR #99 — refactor(engine): extract the inline test modules of candidate, settle and select into sibling files: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, recorded as written |
| **Reviewed SHA** | `33a36a48dc2810b7a315eb3f1170c024e7bcf580` |
| Pull request | eventloops/upstroke#99, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 99` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

## The verdict, verbatim

Blockquoted byte-for-byte from the reviewer's saved output
(`~/review-pr99-33a36a4.md` on the review box).

> ## Finding
>
> The PR body’s byte-for-byte claim is false. [pr.md](/tmp/tmp.36eCpGNbrE/pr.md:271) and its risk section at line 360 say that re-indenting each new child reproduces the original block exactly. Performing that comparison fails for all three:
>
> - `candidate`: one comma differs.
> - `settle`: three commas and a closure-brace pair differ ([pr.diff](/tmp/tmp.36eCpGNbrE/pr.diff:8011)).
> - `select`: one comma differs and the intentionally repointed source path differs.
>
> The subsequent rustfmt section accurately discloses these changes, but that directly contradicts—not qualifies—the absolute byte-identity claim. Replace it with the narrower, supportable claim: token/semantic equivalence after the disclosed formatting transformations and census-path repoint.
>
> I found no runtime failure introduced by the move. The exact [PR #99](https://github.com/eventloops/upstroke/pull/99) diff matches the supplied patch; production-region hashes match, test names/counts and visibility remain unchanged, the registry entries are sorted and resolved, and no stated panic/`anyhow`/path rule is newly violated.
>
> ## Out of scope
>
> - `src/engine/topology/select/tests.rs:1116`: the pre-existing `arm_label` census counts raw braces without blanking comments or strings. A future author can place `// }}` after the existing nine arms, add `Step::Provision => "Provision"` below it, and satisfy compiler-required matches while this census stops at the comment, sees the original nine labels, and passes without classifying or covering `Provision`. The move only repoints the unchanged scanner to the unchanged body, so it does not activate this weakness. [Exact-head source](https://github.com/eventloops/upstroke/blob/33a36a48dc2810b7a315eb3f1170c024e7bcf580/src/engine/topology/select/tests.rs#L1116-L1129).
>
> - `src/effects.rs:1913`: the declaration resolver ignores `cfg_attr` applying `cfg`. With `#[cfg_attr(all(), cfg(test))] mod hidden_tests;`, rustc compiles the child only for tests, while the resolver treats it as unconditional and leaves it in production census domains. This predates the PR and is not activated because all three new declarations use the supported plain `#[cfg(test)]` form. [Exact-head source](https://github.com/eventloops/upstroke/blob/33a36a48dc2810b7a315eb3f1170c024e7bcf580/src/effects.rs#L1913-L1930).
>
> VERDICT: CHANGES_REQUIRED