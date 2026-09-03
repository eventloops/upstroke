# PR #117 — refactor: split agent::proc into per-concern child modules: frontier review record

| field | value |
|---|---|
| **Binding verdict** | **CHANGES_REQUIRED** at `cf51d59`. **This record does NOT bind to the head this pull request merges.** A `CHANGES_REQUIRED` pass is followed by a repair, so the current head is later than the reviewed SHA below; see the delta note for what changed and whether a further pass exists |
| **Reviewed SHA** | `cf51d593243a5ecdae83dfb515f15cf43e95a24e` |
| Passes | 2 , each recorded below as it was returned |
| Pull request | eventloops/upstroke#117, into `master` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 117` |
| Scope | the owner's direction of 2026-09-02: review the change, not the tree. Out-of-scope observations are recorded by the reviewer and carried, not repaired here |

This change went round 2 times. Every pass is recorded, in order, with
its verdict as returned — a completed `CHANGES_REQUIRED` pass is not
folded into the pass that followed it.

**The final pass recorded here returned `CHANGES_REQUIRED`, so no reviewer has
seen the tree at the current head.** The reviewed SHA is `cf51d59`; any
commit after it carries the repair for the findings below and has not
itself been reviewed. This record is evidence of what the reviewer said,
not an attestation of the merge head.

## Pass 1 — `ca571eb` — CHANGES_REQUIRED

Reviewed SHA `ca571ebed32bdc4c2d6f48e8ed5cfd74f7ad053b`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr117-ca571eb.md` on the review box).

> ## Finding
>
> 1. The new allowlist justification is false. `effects/allowlist.toml:231-243` claims the suite performs no effect outside self-created scratch directories and owned process trees ([diff](/tmp/tmp.djnlJ0acuL/pr.diff:45)). But `src/agent/proc/tests.rs:532-547` computes a standalone path directly under `std::env::temp_dir()` and calls `remove_file` before creating any child or owned directory ([diff](/tmp/tmp.djnlJ0acuL/pr.diff:5506)); another test repeats this pattern at lines 1775-1795.
>
>    Concrete sequence: a prior process leaves `upstroke-proc-tree-<pid>-<test-name>.marker`; the OS reuses that PID; the same test runs and deletes the pre-existing file at line 538. That is an effect outside both categories asserted by the new review record. The allowlist gate still passes because it checks the lint set and placement, not the truth of its prose.
>
>    This PR need not repair those old fixtures, but its newly added reviewed artifact must disclose the exceptions instead of asserting they do not exist.
>
> The mechanical extraction otherwise checks out: 69 tests retain their names, all eight visibility changes preserve the effective visibility set, `windows_job` and `termination` are unchanged, and no new production `unwrap`/`expect`, `anyhow`, or non-`std::path` path handling appears.
>
> ## Out of scope
>
> - The predictable-marker ownership weakness itself predates this change at base `src/agent/proc.rs:6059-6074` and `:7309-7332`; the bodies were only moved. Its failure sequence is the PID-reuse deletion described above. Only the new false allowlist claim is in scope here.
> - `src/effects.rs:569-571` and `src/runner/mod.rs:1329-1332` already claimed at the base that `proc.rs` contained eight relevant textual occurrences. The pre-test prefix actually contained six—five code and one comment—at both revisions. The census uses its separate blanked-code count, so this stale prose is not activated by the extraction.
>
> VERDICT: CHANGES_REQUIRED
## Pass 2 — `cf51d59` — CHANGES_REQUIRED

Reviewed SHA `cf51d593243a5ecdae83dfb515f15cf43e95a24e`. Blockquoted byte-for-byte from the reviewer's
saved output (`~/review-pr117-cf51d59.md` on the review box).

> ## Findings
>
> 1. **The PR body is stale and does not support the exact reviewed head.** [`pr.md:72`](/tmp/tmp.CJsVemRecg/pr.md:72) calls `e847e0a` the merged head, and [`pr.md:189`](/tmp/tmp.CJsVemRecg/pr.md:189) records only that earlier base merge. The actual head, `cf51d593`, is a later merge of `a4fdb6d` and `827b3388` with an authored conflict resolution. Its diff hash is `ae91a264…`, matching neither hash recorded at `pr.md:199-200`.
>
>    The omission is material: `a4fdb6d` explicitly repairs “Pass 1 finding 1,” yet the ledger at [`pr.md:254`](/tmp/tmp.CJsVemRecg/pr.md:254) still says “None yet.” The claimed complete branch history at `pr.md:146-152` also stops at `ca571eb`.
>
>    Exact-head evidence does exist: the eight local commands passed at `cf51d59`, and exact-head CI/policy runs `33792513264` and `33792513311` passed, including Windows and macOS. But none is recorded in the body, contrary to the supplied build-box rule requiring the passing head to be recorded before every push.
>
>    Concrete failure sequence: a reviewer finds a false containment claim; `a4fdb6d` repairs it; a subsequent base merge adds reviewed-head movement and a conflict resolution; the body still presents the pre-repair ledger, old hashes, and old validation. A gatekeeper relying on that record cannot distinguish the validated tree from later authored content.
>
> 2. **“Bodies move verbatim” is false as written.** [`pr.md:17`](/tmp/tmp.CJsVemRecg/pr.md:17) says the only moved-body edits are eight visibility prefixes, one qualification, and one comment sentence. There are additional rustfmt rewrites. For example, the base’s single-expression match arm at `src/agent/proc.rs:8996`:
>
>    ```rust
>    other => panic!(...)
>    ```
>
>    became a braced arm at [`src/agent/proc/tests.rs:3455`](/srv/worktrees/w2-m6/src/agent/proc/tests.rs:3455). I found no semantic change in this rewrite, but it is not verbatim and is outside the body’s exhaustive enumeration. The claim should be narrowed to semantic equivalence and disclose the formatting rewrites.
>
> I found no runtime defect caused by the module split: the 69 tests and 1,850-test inventory are conserved; `windows_job` and `termination` compare byte-identically; moved production items differ only by the expected visibility/path edits; public paths and `cfg`s remain intact; lint/effect bookkeeping is present; and the diff adds no production `unwrap`/`expect`, `anyhow`, or non-`std::path` path handling. I could not construct a behavioral failure caused by the split itself.
>
> ## Out of scope
>
> - **Pre-existing unowned temporary-path deletion:** [`src/agent/proc/tests.rs:538`](/srv/worktrees/w2-m6/src/agent/proc/tests.rs:538), `:1784`, `:2171`, and `:2937` remove files or directories before establishing ownership. After a crashed run and PID reuse, the predictable `timeout_kills…` marker can collide with another file; the next run deletes that foreign file. The directory variants can recursively delete a colliding tree. These bodies were moved unchanged, and the new allowlist prose now discloses the hazard, so the split neither introduces nor activates it.
>
> - **Pre-existing raw-source scanner blind spot:** [`src/agent/proc/tests.rs:903`](/srv/worktrees/w2-m6/src/agent/proc/tests.rs:903) searches unparsed source lines for `hooks.point(SubEffectPoint::` and associates the nearest preceding `cfg`. Deleting a real hook call and leaving the same text in a comment under `#[cfg(unix)]` can preserve the expected result while production stops emitting the point. The scanner logic existed unchanged in the former inline test module; extraction merely relocates it.
>
> VERDICT: CHANGES_REQUIRED
