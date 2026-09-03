# PR #123 — refactor(config): split config into per-concern child modules: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED** at `716ed75`. **This record does NOT bind to the head this pull request merges.** A `CHANGES_REQUIRED` pass is followed by a repair, so the current head is later than the reviewed SHA below; see the delta note for what changed and whether a further pass exists |
| **Reviewed SHA** | `716ed754260cff80477b60fd718de57102ccab3b` |
| Passes | 1 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#123, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 123` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 1 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

**The final pass recorded here returned `CHANGES_REQUIRED`, so no reviewer has
seen the tree at the current head.** The reviewed SHA is `716ed75`; any
commit after it carries the repair for the findings below and has not
itself been reviewed. This record is evidence of what the reviewer said,
not an attestation of the merge head.

## Pass 1 — `716ed75` — CHANGES_REQUIRED

Reviewed SHA `716ed754260cff80477b60fd718de57102ccab3b`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr123-716ed75.md` on the review box).

> ## Findings
>
> 1. **Blocking: the verification block records commands that cannot have returned `rc=0`.** [pr.md](/tmp/tmp.alTDzbOCLl/pr.md:17) directly invokes three scripts stored as mode `100644`:
>
>    - `test-pr-ledger-evidence.sh`
>    - `test-pr-policy.sh`
>    - `test-release-record.sh`
>
>    Reproducing those exact commands at SHA `716ed754…` returns “Permission denied”, `rc=126`. The repository’s [CI workflow](/srv/worktrees/w2-m7/.github/workflows/ci.yml:204) correctly invokes them with `bash`. The body must record the commands actually run. Its Cargo lines also omit the build-box-required `upstroke-build` wrapper.
>
>    Fresh exact-head [Ubuntu CI](https://github.com/eventloops/upstroke/actions/runs/33807683711/job/100821948419) does corroborate `1808 passed / 0 failed`; this is an evidence-description defect, not a demonstrated code failure.
>
> 2. **The new read-module contract is literally false.** [pr.diff](/tmp/tmp.alTDzbOCLl/pr.diff:1355) says every function takes `FileSnapshot` and “never a path,” but `parse_pool` explicitly takes `path: &Path` at [pr.diff](/tmp/tmp.alTDzbOCLl/pr.diff:1479). It currently uses that path only for diagnostics, so there is no present TOCTOU bug, but the claimed structural prevention does not exist. Narrow the prose to functions that read file contents, or change the helper boundary.
>
> I found no behavioral regression in the moved code: the function bodies are unchanged apart from `pub(super)` visibility and formatting, only the claimed three files change, the legacy allowlist is untouched, and no new panic, `anyhow`, or path violation appears.
>
> VERDICT: CHANGES_REQUIRED
