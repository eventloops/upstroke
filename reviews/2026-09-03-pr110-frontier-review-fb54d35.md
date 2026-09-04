# PR #110 — refactor: split workspace_manager into per-concern child modules: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED** at `fb54d35`. **This record does NOT bind to the head this pull request merges.** A `CHANGES_REQUIRED` pass is followed by a repair, so the current head is later than the reviewed SHA below; see the delta note for what changed and whether a further pass exists |
| **Reviewed SHA** | `fb54d354fd76f0b1262cb90f30a986b03fc2e2b8` |
| Passes | 2 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#110, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 110` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 2 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

**The final pass recorded here returned `CHANGES_REQUIRED`, so no reviewer has
seen the tree at the current head.** The reviewed SHA is `fb54d35`; any
commit after it carries the repair for the findings below and has not
itself been reviewed. This record is evidence of what the reviewer said,
not an attestation of the merge head.

## Pass 1 — `bab9c0b` — CHANGES_REQUIRED

Reviewed SHA `bab9c0b463e5e8ce5589071f2d1d46d073e9d875`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr110-bab9c0b.md` on the review box).

> ## Findings
>
> 1. The “eleven effect sites” evidence is wrong. [pr.md:37](/tmp/tmp.BmumjbkrbR/pr.md:37) omits `Worktree.Remove`, but the four mapping methods contain 12 distinct site variants ([workspace_manager.rs:376](/srv/worktrees/w2-m4/src/workspace_manager.rs:376)). The reported mutation found only eleven because the census uses substring matching ([effects/tests.rs:2666](/srv/worktrees/w2-m4/src/effects/tests.rs:2666)): after moving the accessors, `WorktreeSite::RemoveExecutionRoot` at [workspace_manager.rs:747](/srv/worktrees/w2-m4/src/workspace_manager.rs:747) falsely satisfies the search for `WorktreeSite::Remove`. Thus “measured rather than assumed” misinterprets a false positive as complete evidence. The underlying scanner defect is pre-existing, but the inaccurate PR claim is in scope.
>
> 2. `parsers.rs` does not contain “three NUL-delimited” decoders as claimed in [pr.md:18](/tmp/tmp.BmumjbkrbR/pr.md:18) and [parsers.rs:3](/srv/worktrees/w2-m4/src/workspace_manager/parsers.rs:3). Only the `worktree list -z` and `diff -z` streams are NUL-delimited. `registration_checkout` trims ASCII whitespace and decodes one textual `gitdir` path. A normal `b"/worktree/.git\n"` is accepted; `b"/worktree/.git\0"` retains the NUL and fails the `.git` filename check. The parser is correct; the newly added documentation must be corrected.
>
> 3. Moving the containment comments changed their meaning even though their bytes were conserved. At [containment.rs:83](/srv/worktrees/w2-m4/src/workspace_manager/containment.rs:83), “every deletion in this module” now refers to a child containing no deletions, making the security assertion vacuous. At [containment.rs:155](/srv/worktrees/w2-m4/src/workspace_manager/containment.rs:155), “whatever this module hands to Git” is false—the child hands paths back to its parent, which invokes Git. These should say “the parent” or “this subsystem.” Otherwise a future unguarded parent deletion would not contradict the literal documentation.
>
> 4. The exact-head multiset evidence is stale. [pr.md:319](/tmp/tmp.BmumjbkrbR/pr.md:319) claims `32 lines out, 375 in` “at this head.” Repeating the stated normalization at `bab9c0b` gives `32 out, 383 in`. `375` was correct through `d8f4d13`; `bab9c0b` added seven comment lines plus `#[cfg(unix)]`. Likewise the test-list table labels `f1918e0` as “head,” although the resulting hash does remain correct at `bab9c0b`.
>
> 5. The rollback claim is false. [pr.md:829](/tmp/tmp.BmumjbkrbR/pr.md:829) says the four commits are revertible “in any order.” Reversing `660e9e1` against the exact head does not apply cleanly: `bab9c0b` modified its new import block in `src/workspace_manager/tests.rs`, while `d8f4d13` modified its new census comments in `src/runner/container/tests.rs`; a three-way inverse produces conflicts. Reverse-chronological rollback is viable, but arbitrary order is not.
>
> The moved production statements, external public surface, lint prologues, and platform compilation otherwise checked out. The isolated full suite, Clippy with denied warnings, and the Windows cross-target check all passed.
>
> ## Out of scope
>
> - The site census’s substring collision at `src/effects/tests.rs:2668` predates this PR and is not activated because the exact `WorktreeSite::Remove` literal remains at `src/workspace_manager.rs:396`. Removing that literal while retaining `RemoveExecutionRoot` would leave the census falsely green. This does not drive the verdict; only the PR body’s incorrect interpretation does.
>
> - The child-lint census still derives modules by walking directories rather than resolving declarations (`src/runner/container/tests.rs:2971`). A `#[path]` relocation plus a compliant decoy file can leave the real child inheriting the parent allowance while every check passes. The walk and this blind spot predate the diff, and none of the new declarations uses `#[path]`.
>
> VERDICT: CHANGES_REQUIRED
## Pass 2 — `fb54d35` — CHANGES_REQUIRED

Reviewed SHA `fb54d354fd76f0b1262cb90f30a986b03fc2e2b8`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr110-fb54d35.md` on the review box).

> ## Finding
>
> 1. The new `SnapshotName` invariant is false. [naming.rs:8](/srv/worktrees/w2-m4/src/workspace_manager/naming.rs:8) says a `SnapshotName` “can only be built by one of its three constructors,” making role/attempt uniqueness structural. But [naming.rs:187](/srv/worktrees/w2-m4/src/workspace_manager/naming.rs:187) directly constructs `SnapshotName(rest.to_owned())` from an intent filename.
>
>    Concrete sequence:
>
>    1. Place `snapshots.shared.intent` in the intent directory; its contents are irrelevant to [WorkspaceManager::intents](/srv/worktrees/w2-m4/src/workspace_manager.rs:855).
>    2. `intents()` returns `Slot::Snapshot { name: SnapshotName("shared") }`; `"shared"` passes `safe_component`.
>    3. Extract the public enum field, remove the seed intent, and pass the name to [add_snapshot](/srv/worktrees/w2-m4/src/workspace_manager.rs:1292).
>    4. Remove that snapshot and reuse the same name for another role or attempt.
>
>    Thus the name need not encode role, generation, or attempt, and reuse still depends on caller discipline. The reconstruction behavior predates this refactor; the change-scoped defect is the newly added categorical claim. Correcting the documentation would remain within this PR’s no-behavior-change scope.
>
> The remaining checked risks passed: moved production bodies were conserved apart from documented visibility/docs changes; effect sites and public paths remained stable; all children state the three lint levels; no new panic, `anyhow`, or non-`std::path` usage appeared. The full all-target suite and Clippy pass, as do strict Windows and Intel-macOS cross-checks; the 1,848-test list and hash match the base.
>
> ## Out of scope
>
> - [containment.rs:83](/srv/worktrees/w2-m4/src/workspace_manager/containment.rs:83) says every subsystem deletion goes through `WorkspaceManager::contained`, but ordinary `remove_intent` reaches `fs::remove_file` directly at [workspace_manager.rs:842](/srv/worktrees/w2-m4/src/workspace_manager.rs:842). The same semantic assertion and deletion path existed at the base; the diff only updates the comment’s module/subsystem referent.
>
> - [effects.rs:1303](/srv/worktrees/w2-m4/src/effects.rs:1303) still claims four `expected_refs(` occurrences in `workspace_manager.rs`. Base and head each contain one there; the other three are in `workspace_manager/tests.rs`. This is the unchanged, body-disclosed C-010 issue.
>
> - A strict rustdoc build still fails, including on the unescaped placeholders at [workspace_manager.rs:7](/srv/worktrees/w2-m4/src/workspace_manager.rs:7). The base fails with the same normalized diagnostic set, so the PR’s narrower claim that the warning set is unchanged is supported.
>
> VERDICT: CHANGES_REQUIRED
