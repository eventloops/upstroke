# PR #40 — frontier review record, `c62ea91`

| field | value |
|---|---|
| **Verdict** | **CHANGES_REQUIRED, five findings** |
| **Reviewed SHA** | `c62ea9189f950aa0d389181902ef45e933b453f9` |
| Pull request | eventloops/upstroke#40, into `codex/parallelism-design` |
| Reviewer | `gpt-5.6-sol`, `model_reasoning_effort=max`, read-only sandbox |
| Transport | `codex exec` via `~/bin/review-pr.sh 40`, 90-minute per-pass limit |
| Driver knobs | `REVIEW_ROOT` (a worktree at the reviewed sha), `REVIEW_PROMPT_EXTRA` |
| CI at the reviewed SHA | GREEN on the newest run per name, after a re-run of a Windows leg cancelled on its thirty-minute job timeout; the other legs succeeded on the first attempt |
| Inputs | the diff, the tree at that sha, the committed prior review records, and the pull request's title and body |

**The most consequential round.** Its first two findings are the only ones in this branch's
history that are about the *design* rather than about the body describing itself, and both
would have shipped into W4:

- the `OsString` record's Consequences ordered adapters to hand a resolved `PathBuf`
  through, which reinstates the defect `PR4-ADAPTER-RESOLVES-ON-THE-HOST` records as
  repaired and contradicts `DESIGN.md:117`. The container case is the concrete one: a CLI
  that exists only inside the image cannot be resolved on the coordinator;
- the same list ordered runner-policy digests to encode the program and named three
  constants for re-pinning. `RunnerPolicy` carries no program and is one identity per run,
  while programs vary per spawn; and those constants pin the topology registry's own digest,
  unrelated to runner policy.

Both are repaired in the record itself, which is unlanded and therefore still correctable,
and the siblings that could be read the same way — the record's grounds and the proposal's
W4 — are narrowed with them.

## The review, verbatim

## Findings

1. **P1 — the `OsString` ruling would reintroduce host-side CLI resolution.** The record claims non-Unicode installations “fail today” and orders adapters to pass a resolved `PathBuf` ([decision](/srv/worktrees/pr40-r8/decisions/2026-08-25-commandspec-program-osstring.md:52)). That is the pre-runner world. Current DESIGN makes adapters produce a data-only command while the runner owns execution ([DESIGN.md](/srv/worktrees/pr40-r8/DESIGN.md:117)); production deliberately emits bare CLI names ([bin.rs](/srv/worktrees/pr40-r8/src/agent/bin.rs:4)), and the runner resolves them to native `PathBuf`s without writing them back into `CommandSpec.program` ([host.rs](/srv/worktrees/pr40-r8/src/runner/host.rs:655), [host.rs](/srv/worktrees/pr40-r8/src/runner/host.rs:729)).

   Concrete failure: `codex` exists only inside the configured container image. Today the adapter emits `codex` and the container boundary resolves it. W4 follows the record, resolves on the coordinator, and either refuses because the host lacks it or passes a host path that does not exist in the container. This recreates `PR4-ADAPTER-RESOLVES-ON-THE-HOST` and contradicts the sole living authority.

2. **P1 — the same record silently expands W4 into an incoherent durable-schema change.** It orders “runner-policy canonical digests” to encode the program and re-pin `HOST/CONTAINER_CANONICAL`, `SAMPLE_DIGEST`, and `SAMPLE_CANONICAL_BYTES` ([decision](/srv/worktrees/pr40-r8/decisions/2026-08-25-commandspec-program-osstring.md:61)). `RunnerPolicy` has no program; it is one durable execution identity per run ([events.rs](/srv/worktrees/pr40-r8/src/topology/events.rs:403)), while programs vary per spawn. The `SAMPLE_*` constants are instead the unrelated topology-registry digest ([registry.rs](/srv/worktrees/pr40-r8/src/topology/registry.rs:2505)).

   An implementer must either add program state to the durable `RunnerPolicy` schema—outside the disclosed W4 scope—or digest an ephemeral resolution that resume cannot reconstruct reliably, causing identity mismatch and refusal. Merely widening `CommandSpec` changes none of those constants. This violates the no-silent-scope-widening rule and contradicts the record’s own claim that widening stops at ephemeral spawn identity.

3. **P2 — the promised deletion was not applied to the exact survivors the previous finding named.** The body claims no counts or mutable state survive outside the generated block, yet still says “one compressed `DESIGN.md` edit” ([pr.md](/srv/worktrees/pr40-r8/pr.md:7)), “runtime risk is zero” ([pr.md](/srv/worktrees/pr40-r8/pr.md:212)), “three earlier revisions” ([pr.md](/srv/worktrees/pr40-r8/pr.md:220)), and “four conventions … three rejected” ([pr.md](/srv/worktrees/pr40-r8/pr.md:233)). `PR40H-STRIP-APPLIED-TO-DIGITS-ONLY` explicitly names these classes and nevertheless marks them fixed ([pr.md](/srv/worktrees/pr40-r8/pr.md:295)). Its asserted checker is neither added by the diff nor effective against the current body.

4. **P2 — a newly added ledger row does not resolve to the defect its identifier claims.** `PR40H-W8-ANCHOR-STILL-IN-W7` binds to proposal line 161 ([pr.md](/srv/worktrees/pr40-r8/pr.md:294)). At `c62ea91`, line 161 is the end of W6; W7 begins at 163 and W8 at 170 ([proposal](/srv/worktrees/pr40-r8/proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md:161)). An auditor follows the row and sees a third workstream, so the “W7” identifier and claimed verification are factually wrong.

5. **P2 — the guard-repair disposition is also false.** `PR40H-GUARDS-DESCRIBE-DELETED-SECTIONS` says every guard now describes the current body ([pr.md](/srv/worktrees/pr40-r8/pr.md:297)). But `PR40E-CI-SECTION-DESCRIBED-AN-EARLIER-HEAD` still says a section heading now names the head of its narrative ([pr.md](/srv/worktrees/pr40-r8/pr.md:276)); that narrative section was deleted. A maintainer following the supposed prevention is directed to content that does not exist.

The repository-derived generated facts do match Git: `pr.diff` is exact, the stat/path and commit ranges recompute, every changed path is Markdown, and `src/**` remains untouched. I found no separate loss of necessary information caused solely by deletion. Consequently, the panic, `anyhow`, and `std::path` rules are not implicated.

VERDICT: CHANGES_REQUIRED