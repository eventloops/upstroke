# PR #41 — frontier review record, `0d994b7`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `0d994b7d3815dac51b6efd6d4d0e74317048e2ef` |
| Pull request | eventloops/upstroke#41, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 41`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, both committed prior review records, and the pull request's title and body |

The third review of #41, and the one that checked the classification rather than the
reasoning behind it. The previous head split the lossy-identity class into failure modes
and put three rows in the wrong ones — a field production replay never reads, a fixture
writer nothing reads back, and a site that renders one basename and then builds a path
from it. It also found a fifteenth row the membership table missed, in the agent settings
path, which is the sharpest instance of the class the table was built to bound.

Its third finding accepts the escalated question as fairly premised — this codebase does
percent-encode paths into strings injectively — and then shows the dispositions attached
to it do not follow, because most of the rows have no `String`-over-`PathBuf` rationale to
extend in the first place, and because the repair is a two-sided representation change
rather than a writer conversion.

## The review, verbatim

Changes are required at exact head `0d994b7d3815dac51b6efd6d4d0e74317048e2ef`.

1. **§1(a)’s failure-mode assignments are wrong.**

   - Work-list row 59 is classified as reconstructing a path from `TaskDispatched.worktree_path`. The field is written in [dispatch.rs](/srv/worktrees/pr41-r3/src/engine/topology/dispatch.rs:398), but production replay ignores it; recovery derives the slot from key and generation in [recover.rs](/srv/worktrees/pr41-r3/src/engine/topology/recover.rs:2638). No path is reconstructed from that string.
   - Row 93 is also class A, but [scaffold.rs](/srv/worktrees/pr41-r3/src/engine/topology/scaffold.rs:151) only creates fixture events. Nothing there reads the strings back or reconstructs a path.
   - Row 258 is not class B. [rundir.rs](/srv/worktrees/pr41-r3/src/rundir.rs:1472) renders one basename lossily; the recorded values it compares against are not independently lossily rendered. It also uses that basename to construct the expected private path and deletion proof at [rundir.rs](/srv/worktrees/pr41-r3/src/rundir.rs:1532). Thus neither “both sides render lossily” nor “only compare” is true.

2. **The fourteen-row membership table omits a fifteenth §8 lossy-identity row.**

   Work-list line 149 reports `src/agent/claude.rs`, where [claude.rs](/srv/worktrees/pr41-r3/src/agent/claude.rs:299) passes `settings.to_string_lossy()` as Claude’s `--settings` target. That is class C: the lossy string selects a target.

   Concrete failure: on Unix, let the intended settings file be under a directory containing raw byte `0x80`, and create a different sibling whose name contains literal U+FFFD. Both render to the sibling’s spelling, so Claude loads the wrong settings file—potentially a more permissive one. The table’s “all fourteen” claim and the ledger’s claim that this repair fixed the class are therefore false.

3. **The escalated question is fairly premised but cannot support its stated dispositions.**

   A `String` can indeed carry a faithful encoding of non-UTF-8 bytes in this codebase: [intent.rs](/srv/worktrees/pr41-r3/src/runner/container/intent.rs:160) percent-encodes paths and [decodes them](/srv/worktrees/pr41-r3/src/runner/container/intent.rs:230). So asking whether a documented `String` choice defends lossiness is reasonable.

   But only the `TaskDispatched` and `RunStarted4` field docs contain the cited `String`-over-`PathBuf` rationale. Rows 51/264 (`CreatingMarker.private_dir`), 198/224 (legacy `RunStarted` fields), and 201 (`repo_relative`) lack that rationale. Consequently, the outcome in [§1(a)](/srv/worktrees/pr41-r3/reviews/2026-08-28-pr7-standards-triage.md:217)—that a “yes” returns class A to rejected—does not follow.

   The later claim that repair is merely changing the conversion call site is also false. Encoded strings require decoding at consumers such as `PathBuf::from`; otherwise byte `0x80` becomes the literal path `%80`. Adding decoding without a tagged/versioned representation would reinterpret historical paths literally named `%80`, conflicting with §8’s compatibility requirement. This needs a two-sided, backward-compatible representation, not just a different writer conversion.

4. **The §9 row was not removed from §1(c) as claimed.**

   The body and triage say `exec_streams` moved out of the sweep list, but it remains in [§1(c)’s table](/srv/worktrees/pr41-r3/reviews/2026-08-28-pr7-standards-triage.md:316). That makes the preceding statement that “all nine cite §14” false because this retained row cites §9. The routing rationale itself checks out: §9 is MUST-tagged, while [exec_streams](/srv/worktrees/pr41-r3/src/runner/container.rs:1506) documents stream separation but supplies no timeout or output bound. The two “unaudited” cells currently still identify the intended table rows.

5. **The stated scope is false.**

   [pr.md](/srv/worktrees/pr41-r3/pr.md:86) claims three changed files and lists only the first review record. The exact diff changes four files, adding `reviews/2026-08-28-pr41-frontier-review-788c714.md` as well. A later section acknowledges both reviews, so it is not globally concealed, but the explicit scope boundary and singular rollback reference are stale and do not match the diff exactly.

Checks that held: `pr.diff` matches the exact base-to-head diff; no `src/**`, decision, workflow, configuration, or `DESIGN.md` file changed. The §0b authority quotation matches `decisions/README.md`, and current normative text does not treat a decision record or review as living authority. All twelve ledger locations resolve at their respective `ea25033` or `788c714` SHA. The other §1(c) verdicts were supportable or honestly marked unaudited, and the absent-lens-artifact disclosure is accurate.

VERDICT: CHANGES_REQUIRED