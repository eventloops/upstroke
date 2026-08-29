# PR #40 — frontier review record, `1de9131`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, four findings** |
| **Reviewed SHA** | `1de913109e72248383fcced5271bf3d8556f01ae` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, selected by run id — `CI` run `33198420688`, `Pull request policy` run `33198420634` |
| Inputs | the diff, the tree at that sha, the seven committed prior review records, and the pull request's title and body |

Its third finding is the most consequential this branch has received, and it lands on the
repair the previous round made rather than on the defect that round fixed. The `OsString`
record's remaining rationale — that a non-Unicode installation fails today, and that the
widening serves a CLI named by path — depends on a production route that does not exist:
`Invocation::at` is `#[cfg(test)]` and says so in its own doc, production's only constructor
takes a bare name, and `runner/mod.rs` states that **a `String` was always wide enough** for
that name while `host.rs` resolves it through the native `PATH` as an `OsStr`. So a CLI under
a non-UTF-8 directory is found and executed today, and no `CommandSpec` refusal occurs.

That is a question about the ruling rather than about its wording, and it is **carried to the
owner unresolved** rather than answered here.

## The review, verbatim

Changes required. I found four blocking defects.

1. **P1 — the generated block is not for the exact head.** It identifies `c62ea91` as its generation point ([pr.md](/srv/worktrees/pr40-r10/pr.md:35)). At `1de9131`, Git reports 15 files and 1,050 insertions—not 14/966; 15/27 commits in the two ranges—not 12/24; and eight review records with 44 findings—not seven/39. The path list omits [the `c62ea91` review record](/srv/worktrees/pr40-r10/reviews/2026-08-28-pr40-frontier-review-c62ea91.md:1), while the commit list omits `df0d954`, `5514e2e`, and `1de9131`. The CI table likewise evidences `c62ea91`, not this head. Sequence: a maintainer trusts “CI at this head” and the generated scope → omits the newest review and accepts checks from an older tree → the exact-head gate is not established.

2. **P1 — every row added for the previous review binds to the wrong SHA.** The committed review says it examined `c62ea9189…` ([record](/srv/worktrees/pr40-r10/reviews/2026-08-28-pr40-frontier-review-c62ea91.md:6)), but all five `PR40I-*` rows bind to repaired, unreviewed `1de9131` ([pr.md](/srv/worktrees/pr40-r10/pr.md:299)). That contradicts both the ledger preamble ([pr.md](/srv/worktrees/pr40-r10/pr.md:230)) and `MAINTAINING.md`’s exact reviewed-SHA/file-line rule. The first two rows point to source evidence rather than the defective decision lines. Worse, `PR40I-NEW-ROW-CITED-ANOTHER-SHAS-LINE` claims its anchor was resolved correctly, but current proposal line 170 is still W7; W8 begins at [line 173](/srv/worktrees/pr40-r10/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:173). It also omits the recurring prior ID `PR40H-W8-ANCHOR-STILL-IN-W7`. An auditor follows these rows and sees repaired code or the wrong workstream, making the original findings disappear.

3. **P1 — the repaired `OsString` rationale now depends on a production route that does not exist.** The record still claims a non-Unicode installation “fails today” and replaces the rejected host-resolution rationale with an adapter or configuration naming a CLI by path ([decision](/srv/worktrees/pr40-r10/decisions/2026-08-25-commandspec-program-osstring.md:50), [decision](/srv/worktrees/pr40-r10/decisions/2026-08-25-commandspec-program-osstring.md:73)). But production’s only `Invocation` constructor is the bare-name `Invocation::named`; `Invocation::at` is test-only ([bin.rs](/srv/worktrees/pr40-r10/src/agent/bin.rs:225)). The runner explicitly says a `String` is wide enough for that name ([runner/mod.rs](/srv/worktrees/pr40-r10/src/runner/mod.rs:72)) and resolves it through the native `OsString` `PATH` into a `PathBuf` ([host.rs](/srv/worktrees/pr40-r10/src/runner/host.rs:1045)). Concrete case: place `claude` under a non-UTF-8 Unix `PATH` directory → adapter emits `"claude"` → runner joins that name to the native path and executes it → no `CommandSpec` refusal occurs. W4’s mandated widening and test deletion therefore change no production behavior. Making the claimed benefit real would require adding a new path-valued adapter/configuration input, silently widening W4 beyond its enumerated scope.

4. **P2 — the promised deletion was again implemented as paraphrasing.** The prior review’s “one compressed edit” survives as “a compressed edit” ([pr.md](/srv/worktrees/pr40-r10/pr.md:7)); “runtime risk is zero” survives as “Documentation only … risk … not in what the engine does” ([pr.md](/srv/worktrees/pr40-r10/pr.md:212)); chronology survives as “round after round, every commit invalidated several” ([pr.md](/srv/worktrees/pr40-r10/pr.md:23)); and the counted conventions survive as “Successive conventions have been tried and rejected” ([pr.md](/srv/worktrees/pr40-r10/pr.md:234)). Other mutable state remains outside the block, including the local binary-check outcome ([pr.md](/srv/worktrees/pr40-r10/pr.md:163)) and the title’s universal “every prior round’s record.” Thus `PR40I-SURVIVORS-NAMED-AND-NOT-REMOVED` falsely says every named sentence was removed ([pr.md](/srv/worktrees/pr40-r10/pr.md:303)).

I found no necessary information lost through deletion: inventories, history, CI facts, and review metadata remain recoverable from Git, workflow data, or committed records. I also confirmed `pr.diff` exactly matches Git, all actual changed paths are Markdown, `src/**` is untouched, and the panic/`anyhow`/`std::path` rules are not implicated. The decision files are additions, so landed-record immutability is not yet violated.

VERDICT: CHANGES_REQUIRED