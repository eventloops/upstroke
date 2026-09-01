# PR #94 — the census W0 doc-fix packet: frontier review record

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED**, three findings, recorded as written. All three repaired in the one documentation commit above the reviewed head, disclosed below; none meets the serious-P1 bar |
| **Reviewed SHA** | `1e54aee70947d775682ec6957f208268c22aa2dd` |
| Pull request | eventloops/upstroke#94, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 94`, 90-minute per-pass limit |
| Inputs | the 143-line exact-head diff (5 files) assembled from the PR's own base `c3e6ea8`, and the body |
| CI at the reviewed SHA | 11/11 success, uncancelled: `upstroke-ci` run 33565505815 (10 jobs) and `upstroke-pr-policy` run 33565505820 |
| Pass history | pass 1 `1e54aee`, three findings (this record): the pull request's one full pass under `decisions/2026-09-01-review-effort-rescoped.md` |

One full pass ran under the re-scoped rule; its findings were triaged per
`MAINTAINING.md` step 6 by the owner's agent under the owner's standing
direction of 2026-09-01. All three are factual corrections to documentation
(a licence characterisation the tagged artifact contradicts, a landing date
one day early, a doc link to the wrong test); none reaches the bar, each was
verified against the object database before repair, and no second pass ran.
The repairs sit in one documentation-only commit above the reviewed head;
the delta is listed in the body for the owner's read before merge, and the
owner's merge attests it.

## The verdict, verbatim

Blockquoted byte-for-byte from the reviewer's saved output
(`~/review-pr94-1e54aee.md` on the review box, sha256 `d9bc41dd8a471865…`).

> Changes required. I found three factual defects.
>
> 1. `CHANGELOG.md:5-6` overstates the historical licence. The immutable `v0.1.0` tag’s `src/lib.rs:7-8` permits “version 3 … or any later version,” while its Cargo metadata and README say `AGPL-3.0-only`. Commit `9baad9c` explicitly aligned that notice only on 2026-08-13—after the tag. SPDX treats “only” and “or later” as distinct choices determined by the file notice. [SPDX](https://spdx.org/licenses/AGPL-3.0-only.html) A compliance audit following the new changelog therefore encounters contradictory terms in the tagged artifact. The blanket `AGPL-3.0-only` claim is unsupported and should be qualified.
>
> 2. `CLAUDE.md:43`, `DESIGN.md:302`, `DESIGN.md:561`, and the PR body give the wrong landing date for `export-decisions`. The contract commit `a297d503` and implementation commit `28b0a1e3` are both dated 2026-08-12; the latter creates `src/export.rs` and wires the CLI, and its parent has neither. Concrete failure: check out the final 2026-08-11 head and attempt the documented command—the subcommand does not exist. This makes the sole living authority historically false. Say “decided 2026-08-11, landed 2026-08-12” or equivalent.
>
> 3. `src/runner/container/view.rs:39` now names an existing but wrong test. `exec.rs:6532` checks Git-visible behavior—HEAD, refs, index, and object resolution—but never asserts the absence of `commondir`, `gitdir`, `worktrees/`, remotes, URLs, or credential helpers “by name.” Adding an unused remote or credential-helper stanza leaves that Docker test green. The actual name-based census is `view::tests::the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository` at `view.rs:1082`, with the relevant assertions at lines 1107-1129. The link should point there.
>
> The patch otherwise changes exactly the five declared paths, preserves DESIGN line counts, and adds no unwrap/expect, anyhow, or path-handling code.
>
> VERDICT: CHANGES_REQUIRED
## Disposition of the three findings

1. Fixed. The `v0.1.0` tag's `src/lib.rs` notice reads "or (at your option)
   any later version" while its Cargo metadata reads `AGPL-3.0-only`, so the
   changelog no longer characterises the earlier terms; it says earlier
   releases keep the terms recorded in their own tagged metadata and source
   notices. Ledger row `PR94-CHANGELOG-OVERSTATES-OLD-LICENCE`, fixed.
2. Fixed. `src/export.rs` first landed in `28b0a1e` on 2026-08-12, the
   decision record in `a297d50` the evening before; CLAUDE.md, DESIGN.md
   line 302 and DESIGN.md line 561 now say decided 2026-08-11, landed
   2026-08-12, still in place with the line count unchanged. Ledger row
   `PR94-EXPORT-LANDING-DATE-EARLY`, fixed.
3. Fixed. The name-based census is
   `the_role_view_carries_no_engine_refs_and_no_link_back_into_the_repository`
   in `view.rs`'s own tests module (line 1082 at the reviewed head); the
   doc link now points there. Ledger row `PR94-VIEW-DOC-LINK-WRONG-TEST`,
   fixed.
