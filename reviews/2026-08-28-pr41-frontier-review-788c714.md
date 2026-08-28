# PR #41 — frontier review record, `788c714`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, six findings** |
| **Reviewed SHA** | `788c714785f228cd74ab9ab4696136206c4b5dcf` |
| Pull request | eventloops/upstroke#41, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 41`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the committed record of the first review, and the pull request's title and body |

The second review of #41, and the one that checked the **call sites** rather than the
docs. Its second finding is the consequential one: the byte-`0x80` replay sequence does
not fit `canonical_string`, because nothing reconstructs a path from the `public_dir` it
writes — recovery derives that directory from `repo_root` and the run id and renders it
lossily on both sides. The sequence fits `RunStarted4.private_dir`, which the same owner
amendment left rejected as compliant. Its third finding shows why that is not a
one-site correction: the field docs argue for `String` over `PathBuf`, not for
`to_string_lossy`, so the test the amendment states does not stop where either revision
stopped it.

Its first finding is the routing test never having been applied to §9, and its fourth is
this seat's authority sentence contradicting `decisions/README.md`, which reserves living
authority to `DESIGN.md` alone.

## The review, verbatim

Changes are required at `788c714785f228cd74ab9ab4696136206c4b5dcf`.

1. **P1 — The restored subprocess site is misclassified as a §14 SHOULD.** The triage says all nine unbounded-input sites cite §14 and may remain sweep work ([triage:251](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr7-standards-triage.md:251)), but `exec_streams`’s filed row cites §9 ([worklist:272](/srv/worktrees/pr41-r2/reviews/2026-08-25-pr7-standards-worklist.md:272)). Section 9 says every subprocess integration **MUST** define and test timeout/cancellation and stdout/stderr size behavior ([CODING_STANDARDS.md:320](/srv/worktrees/pr41-r2/CODING_STANDARDS.md:320)). The implementation uses unbounded `Command::output()` ([container.rs:1528](/srv/worktrees/pr41-r2/src/runner/container.rs:1528)). Sequence: `docker logs` hangs or emits unlimited output → `output()` waits indefinitely or buffers toward OOM → the triage incorrectly leaves a MUST violation without the named owner required for immediate correctness/liveness risk. The evidence note is also stale: there are eight table rows; the sixth and eighth are marked unaudited, not “the last two” ([triage:269](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr7-standards-triage.md:269)).

2. **P1 — The byte-`0x80` replay failure is assigned to the wrong lossy-path finding.** `canonical_string` does write `public_dir` and lossily renders it, but recovery does not reconstruct a path from that record. It derives the real `public_dir` from `repo_root` and `run_id` ([recover.rs:279](/srv/worktrees/pr41-r2/src/engine/topology/recover.rs:279)), lossily renders that real path again, and compares strings ([recover.rs:632](/srv/worktrees/pr41-r2/src/engine/topology/recover.rs:632)). Both sides therefore produce U+FFFD; no different public path is reconstructed. The actual stated sequence exists for `RunStarted4.private_dir`: creation writes it with `to_string_lossy` ([create.rs:1647](/srv/worktrees/pr41-r2/src/engine/topology/create.rs:1647)), then recovery executes `PathBuf::from(&started.private_dir)` ([recover.rs:335](/srv/worktrees/pr41-r2/src/engine/topology/recover.rs:335)). That member remains among the twelve rejected as compliant. Thus the claimed boundary and DESIGN §4 failure in [triage:163](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr7-standards-triage.md:163) are backwards.

3. **P1 — The “13 findings / 12 documented deviations” arithmetic remains unsupported.** The filed table contains fourteen §8 observations matching the carried lossy-path family at lines 47, 50, 51, 59, 93, 112, 156, 198, 201, 224, 237, 258, 264, and 267. The triage supplies no per-row membership table explaining which is excluded. Its rationale cannot cover the inherited `agent/codex.rs`, `gates.rs`, and container-mount rows: those are subprocess arguments, executable probing, and a Docker mount source—not `RunStarted4` or `TaskDispatched` fields ([worklist:156](/srv/worktrees/pr41-r2/reviews/2026-08-25-pr7-standards-worklist.md:156), [worklist:237](/srv/worktrees/pr41-r2/reviews/2026-08-25-pr7-standards-worklist.md:237), [worklist:267](/srv/worktrees/pr41-r2/reviews/2026-08-25-pr7-standards-worklist.md:267)). A Unix bind source containing byte `0x80` is still replaced before Docker sees it, selecting a different/nonexistent source. Likewise, `scaffold.rs` independently chooses `.to_string_lossy()` ([scaffold.rs:151](/srv/worktrees/pr41-r2/src/engine/topology/scaffold.rs:151)); a field-level reason for using `String` does not explain why replacing OS-native identity bytes is acceptable. The amendment’s claim that scaffold makes “no identity decision” is false.

4. **P1 — The authority amendment contradicts the project’s sole-authority rule.** The triage says rulings are “constituted by” decision records and that living authority is reserved to “DESIGN.md and the records” ([triage:70](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr7-standards-triage.md:70)). `decisions/README.md` says the opposite: DESIGN is the only living authority and records are history, not spec ([decisions/README.md:8](/srv/worktrees/pr41-r2/decisions/README.md:8)). The PR body repeats the incorrect joint-authority claim ([pr.md:163](/srv/worktrees/pr41-r2/pr.md:163)), while the triage declares that durable log identity stays `String` ([triage:305](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr7-standards-triage.md:305)) without a corresponding DESIGN change. Hard merge order cannot repair that semantic violation.

5. **P2 — All six ledger rows cite a SHA that was never reviewed.** The body correctly says the first review examined `ea25033` and that a fresh review of this head was owed ([pr.md:122](/srv/worktrees/pr41-r2/pr.md:122)); the committed review confirms `ea25033…` ([review record:6](/srv/worktrees/pr41-r2/reviews/2026-08-28-pr41-frontier-review-ea25033.md:6)). Yet every ledger row names `788c714…` as its reviewed SHA ([pr.md:193](/srv/worktrees/pr41-r2/pr.md:193)). That violates the exact-reviewed-SHA requirement in [MAINTAINING.md:67](/srv/worktrees/pr41-r2/MAINTAINING.md:67) and falsely binds old failure sequences to their repaired locations.

6. **P2 — “Three claims are seat-attested” is another stale count.** Immediately before that sentence, fidelity to the lenses is explicitly another seat-attested claim ([pr.md:97](/srv/worktrees/pr41-r2/pr.md:97)). “No lens produced a diff” and the current-head gate-log/embedded-binary claims also depend on absent logs ([pr.md:76](/srv/worktrees/pr41-r2/pr.md:76), [pr.md:116](/srv/worktrees/pr41-r2/pr.md:116)). The disclosure therefore understates how many claims cannot be verified from this repository.

What did check out: the exact diff changes only the three claimed documentation files; the tables contain 321 rows split 101/220; all fourteen lens totals and the 96-file count reconcile; all routed digests recompute under the stated newline scheme; five routed rows cite the applicable MUST clauses in §14 and two cite §8; `read_file_bounded` documents its deliberate non-cap and the incremental log poll really is its module’s exception. The seven #42 IDs are absent but honestly disclosed as forward references. The global “zero findings propose an edit” and mechanical-fidelity claims are withdrawn; the remaining “None proposed” statement is scoped to frozen-layer rows.

VERDICT: CHANGES_REQUIRED