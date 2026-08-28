# PR #43 — frontier review record, `de901a0`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, six findings** |
| **Reviewed SHA** | `de901a096756edde75970f16c131767e87767409` |
| Pull request | eventloops/upstroke#43, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 43`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id |
| Inputs | the diff, the tree at that sha, the two preserved CI logs, and the pull request's title and body |

The first review of #43, and it refuted the central claim of the record it reviewed by
reading one word more carefully than three previous readings had. `exit status: 143` is
an exit **code**; Rust prints `signal: 15 (SIGTERM)` for a signal death. So the helper
was not killed by the test's signal, and the mechanism this branch exists to record was
wrong for the third time.

It also found that both records cited `CODING_STANDARDS.md` §12 for a flake-rate rule
that section does not contain, and that the macOS non-attribution argument is not
structural, because `DESIGN.md` and `decisions/README.md` are compiled into the test
binary by `include_str!`.

## The review, verbatim

## Findings

1. The macOS record’s central interpretation is factually wrong. The log says `exit status: 143`, not signal termination ([log](/home/ubuntu/tactus-artifacts/flakes/2026-08-28-macos-proc-signal-attempt1-run33162906210.log:1770)). Rust’s Unix formatter emits `exit status: N` for an exit code and `signal: 15 (SIGTERM)` for SIGTERM termination ([official source](https://doc.rust-lang.org/stable/src/std/sys/process/unix/unix.rs.html#1255-1265)). The tree even has an explicit `_exit(128 + terminating)` path capable of producing code 143 ([proc.rs](/srv/worktrees/pr43-r1/src/agent/proc.rs:2298)). Thus the source proves only that the helper exited within the outer bound; it does not prove it was “killed by the signal the test sent.” The subsequent claims of a guard-mask race, “not runner load,” and proximity to a real code defect are unsupported. Moreover, guard initialization and unblocking finish before the supervised child is spawned and before the test later sends SIGTERM, so the specifically named initialization race does not fit the sequence. This directly contradicts the PR body’s claim that neither record states a cause or defect ([pr.md](/srv/worktrees/pr43-r1/pr.md:115)) and silently widens the promised fingerprint-only scope.

2. The Windows record likewise asserts a cause its evidence cannot identify. On Windows, `died_by_abort` merely rejects exit code 101 ([workspace_manager.rs](/srv/worktrees/pr43-r1/src/workspace_manager.rs:3875)), while `run_kill_child` discards the child’s stdout and stderr ([workspace_manager.rs](/srv/worktrees/pr43-r1/src/workspace_manager.rs:3770)). Any child panic therefore produces the same parent assertion. Concrete sequence: `settle_retry` returns a non-`Start` result; the child panics at line 1261 before the kill is armed; it exits 101; the parent emits the exact recorded Message B at scaffold line 1356. The record would misclassify that distinct regression as “the injection stopped killing” and “ran past the kill site” ([Windows record](/srv/worktrees/pr43-r1/reviews/2026-08-28-windows-topology-kill-single-failure.md:46)). Message A has the opposite matching problem: its purported fingerprint embeds process-specific `7784`, with no normalization rule. The promised “next red is judgeable” property therefore fails in both false-positive and false-negative directions.

3. “No rate” and `n=1` contradict the records’ own evidence. The macOS record observes one failed attempt plus a green rerun, i.e. 1/2 among the attempts it cites. The Windows record observes two earlier successful same-source Windows jobs plus this failure, i.e. 1/3 under its own equivalence premise ([Windows record](/srv/worktrees/pr43-r1/reviews/2026-08-28-windows-topology-kill-single-failure.md:71)). Those are small, opportunistic rates and do not establish a cause, but they are numerators over denominators of observed runs. If the sibling runs are not comparable, they cannot serve as the claimed control either.

4. The cited §12 policy does not exist at the reviewed head. [`CODING_STANDARDS.md` §12](/srv/worktrees/pr43-r1/CODING_STANDARDS.md:402) contains deterministic-test and census rules and ends at line 469; it contains none of the claimed numerator/denominator, fingerprint, rerun, or later-red rules. Consequently every “§12 requires/forbids/makes” assertion is unsupported, and the claim that #43 “orders anywhere” is false unless the absent standards change lands first and this PR is updated. As written, non-authoritative review records are silently installing a future CI-triage policy.

5. The macOS non-attribution argument is not structural. Although #40 changed only Markdown, it changed `DESIGN.md` and `decisions/README.md`; both are compiled into the library test binary with `include_str!` ([export.rs](/srv/worktrees/pr43-r1/src/export.rs:1091), [export.rs](/srv/worktrees/pr43-r1/src/export.rs:1151)). Therefore `c3e5b20` did not run a byte-identical test binary. The changed strings are scanned by tests in that binary, and those tests run alongside the timing-sensitive signal test. Binary layout or concurrent workload provides a concrete causal path by which a Markdown change can alter whether a race manifests. This does not prove #40 caused the red, but it defeats “provably could not.” The corresponding Windows path check is stronger because the added review file is not compiled or read by the suite.

6. The rollback claim is false. There are four commits, not two; the macOS file was added and then modified twice. Reversing its adding commit against this head does not apply cleanly. The Windows record also repeatedly refers to its companion’s content, contradicting “neither refers to the other’s content” ([pr.md](/srv/worktrees/pr43-r1/pr.md:138)).

The literal scope and raw log fidelity did check out: `pr.diff` exactly matches two Markdown additions; no frozen source, `DESIGN.md`, or decision record is changed; the byte counts, Windows MD5, quoted panic lines, assertion sites, and suite totals match the preserved logs. #40’s 11-Markdown-file/830-insertion count and the identical `src/` trees for #41, #42, and `02b7399` also check out. GitHub head associations, sibling job conclusions/times, and the claimed local eight-gate run remain unverifiable without the CI/API evidence.

VERDICT: CHANGES_REQUIRED